//! DioRouter end-to-end over an in-process Dio: plain GETs serve the cache
//! without fetching, detail GETs hydrate once, watches stream ADDED then
//! MODIFIED lines as augmentation lands, and a closed watch connection
//! releases its scenery.
#![cfg(feature = "axum")]

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use ciborium::Value as CborValue;
use futures_util::StreamExt;
use tempfile::TempDir;
use tower::ServiceExt;
use vantage_api_adapters::axum_dio::DioRouter;
use vantage_diorama::{Augmentation, Detail, Dio, Fetch, Lens, MergeRule, Source};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};
use vantage_vista_factory::VistaCatalog;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), text(v)))
        .collect()
}

fn meta(columns: &[&str]) -> VistaMetadata {
    let mut m = VistaMetadata::new();
    for c in columns {
        let col = if *c == "id" {
            Column::new("id", "String").with_flag("id")
        } else {
            Column::new(*c, "String")
        };
        m = m.with_column(col);
    }
    m.with_id_column("id")
}

/// Gated, counting detail shell (same shape as the diorama scheduler tests):
/// each `get` logs and then waits for a semaphore permit.
mod gated {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use ciborium::Value as CborValue;
    use indexmap::IndexMap;
    use tokio::sync::Semaphore;
    use vantage_core::Result;
    use vantage_types::Record;
    use vantage_vista::capabilities::VistaCapabilities;
    use vantage_vista::metadata::VistaMetadata;
    use vantage_vista::reference::Reference;
    use vantage_vista::source::TableShell;
    use vantage_vista::{Column, Vista};

    pub struct GatedDetailShell {
        pub rows: Arc<IndexMap<String, Record<CborValue>>>,
        pub gets: Arc<AtomicUsize>,
        pub log: Arc<Mutex<Vec<String>>>,
        pub gate: Arc<Semaphore>,
        metadata: VistaMetadata,
        capabilities: VistaCapabilities,
    }

    impl GatedDetailShell {
        /// `open` permits pre-loaded; pass a large number for an ungated shell.
        pub fn new(rows: IndexMap<String, Record<CborValue>>, open: usize) -> Self {
            let metadata = VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("size", "String"))
                .with_id_column("id");
            Self {
                rows: Arc::new(rows),
                gets: Arc::new(AtomicUsize::new(0)),
                log: Arc::new(Mutex::new(Vec::new())),
                gate: Arc::new(Semaphore::new(open)),
                metadata,
                capabilities: VistaCapabilities::default(),
            }
        }
    }

    #[async_trait]
    #[allow(clippy::ptr_arg)]
    impl TableShell for GatedDetailShell {
        fn columns(&self) -> &IndexMap<String, Column> {
            &self.metadata.columns
        }
        fn references(&self) -> &IndexMap<String, Reference> {
            &self.metadata.references
        }
        fn id_column(&self) -> Option<&str> {
            self.metadata.id_column.as_deref()
        }
        async fn list_vista_values(
            &self,
            _vista: &Vista,
        ) -> Result<IndexMap<String, Record<CborValue>>> {
            Ok(IndexMap::new())
        }
        async fn get_vista_value(
            &self,
            _vista: &Vista,
            id: &String,
        ) -> Result<Option<Record<CborValue>>> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            self.log.lock().unwrap().push(id.clone());
            let permit = self.gate.acquire().await.expect("gate open");
            permit.forget();
            Ok(self.rows.get(id).cloned())
        }
        async fn get_vista_some_value(
            &self,
            _vista: &Vista,
        ) -> Result<Option<(String, Record<CborValue>)>> {
            Ok(None)
        }
        fn capabilities(&self) -> &VistaCapabilities {
            &self.capabilities
        }
        fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
            Some(Box::new(Self {
                rows: self.rows.clone(),
                gets: self.gets.clone(),
                log: self.log.clone(),
                gate: self.gate.clone(),
                metadata: self.metadata.clone(),
                capabilities: self.capabilities.clone(),
            }))
        }
        fn driver_name(&self) -> &'static str {
            "gated-detail"
        }
    }
}

struct Fixture {
    dio: Dio,
    router: Router,
    gets: Arc<std::sync::atomic::AtomicUsize>,
    gate: Arc<tokio::sync::Semaphore>,
    _tmp: TempDir,
}

/// Dio over two master rows (id, modified) augmented with a `size` detail;
/// the cache is pre-seeded the way a server's `on_start` sync would.
async fn fixture(open_permits: usize) -> Fixture {
    let tmp = TempDir::new().unwrap();

    let master_rows = [
        ("r0", record(&[("id", "r0"), ("modified", "t1")])),
        ("r1", record(&[("id", "r1"), ("modified", "t1")])),
    ];
    let mut master = MockShell::new();
    for (id, rec) in &master_rows {
        master = master.with_record(*id, rec.clone());
    }
    let master = Vista::new(
        "files",
        Box::new(master.with_metadata(meta(&["id", "modified"]))),
    );

    let mut detail_rows = indexmap::IndexMap::new();
    detail_rows.insert("r0".to_string(), record(&[("id", "r0"), ("size", "100")]));
    detail_rows.insert("r1".to_string(), record(&[("id", "r1"), ("size", "200")]));
    let detail = gated::GatedDetailShell::new(detail_rows, open_permits);
    let gets = detail.gets.clone();
    let gate = detail.gate.clone();

    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .viewport_debounce(Duration::from_millis(1))
            .build()
            .expect("lens builds"),
    );
    let dio = lens.make_dio(master).await.expect("make_dio").augment(
        Arc::new(VistaCatalog::new()),
        vec![Augmentation {
            detail: Detail::Fixed(Arc::new(Vista::new("sizes", Box::new(detail)))),
            source: Source::Id,
            fetch: Fetch::PerRow,
            merge: MergeRule {
                columns: vec!["size".into()],
            },
        }],
    );

    // Seed the cache like a startup sync pump would.
    let seed: indexmap::IndexMap<String, Record<CborValue>> = master_rows
        .iter()
        .map(|(id, rec)| (id.to_string(), rec.clone()))
        .collect();
    dio.cache().insert_values(seed).await.expect("seed cache");

    let router = DioRouter::new(dio.clone())
        .with_column("name", "id")
        .with_column("modified", "modified")
        .with_column("size", "size")
        .with_page_size(10)
        .into_router();

    Fixture {
        dio,
        router,
        gets,
        gate,
        _tmp: tmp,
    }
}

async fn get_json(router: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap()
        .to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, value)
}

/// Open a streaming request and return its NDJSON line reader.
async fn get_stream(router: &Router, uri: &str) -> LineReader {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/x-ndjson",
        "watch responses are NDJSON"
    );
    LineReader {
        stream: Box::pin(response.into_body().into_data_stream()),
        buffer: String::new(),
    }
}

struct LineReader {
    stream: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Send>,
    >,
    buffer: String,
}

impl LineReader {
    /// Next event line, parsed. Panics after two seconds of silence.
    async fn next(&mut self) -> serde_json::Value {
        loop {
            if let Some(pos) = self.buffer.find('\n') {
                let line: String = self.buffer.drain(..=pos).collect();
                return serde_json::from_str(line.trim()).expect("valid event JSON");
            }
            let chunk = tokio::time::timeout(Duration::from_secs(2), self.stream.next())
                .await
                .expect("stream produced a line in time")
                .expect("stream still open")
                .expect("no body error");
            self.buffer.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }
}

async fn eventually(label: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition '{label}' not met within timeout");
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_listing_returns_cache_window_without_fetching() {
    let fx = fixture(0).await; // gate closed — any fetch would hang, none may happen

    let (status, body) = get_json(&fx.router, "/?offset=0&limit=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], "r0");
    assert_eq!(items[0]["modified"], "t1");
    assert_eq!(items[0]["size"], serde_json::Value::Null, "not hydrated");
    assert_eq!(
        fx.gets.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a plain GET never fetches details"
    );

    // Windowing applies.
    let (_, page2) = get_json(&fx.router, "/?offset=1&limit=1").await;
    let items = page2["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "r1");
}

#[tokio::test]
async fn get_detail_hydrates_once_and_serves_cache_after() {
    let fx = fixture(1024).await; // gate open

    let (status, body) = get_json(&fx.router, "/r0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["size"], "100", "detail GET returns the hydrated row");
    assert_eq!(body["modified"], "t1", "cheap columns survive");
    assert_eq!(fx.gets.load(std::sync::atomic::Ordering::SeqCst), 1);

    let (_, again) = get_json(&fx.router, "/r0").await;
    assert_eq!(again["size"], "100");
    assert_eq!(
        fx.gets.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "repeat detail GET is a cache hit"
    );

    let (missing, _) = get_json(&fx.router, "/nope").await;
    assert_eq!(missing, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn watch_listing_streams_added_then_modified_with_augment() {
    let fx = fixture(0).await; // hold hydration until the ADDED lines are read

    let mut lines = get_stream(&fx.router, "/?offset=0&limit=2&watch=true").await;

    // Initial sweep: every visible row arrives as ADDED, un-hydrated.
    let first = lines.next().await;
    assert_eq!(first["type"], "ADDED");
    assert_eq!(first["object"]["name"], "r0");
    assert_eq!(first["object"]["size"], serde_json::Value::Null);
    let second = lines.next().await;
    assert_eq!(second["type"], "ADDED");
    assert_eq!(second["object"]["name"], "r1");

    // Release augmentation: each hydrated row streams in as MODIFIED.
    fx.gate.add_permits(64);
    let mut sizes = std::collections::BTreeMap::new();
    for _ in 0..2 {
        let event = lines.next().await;
        assert_eq!(event["type"], "MODIFIED");
        sizes.insert(
            event["object"]["name"].as_str().unwrap().to_string(),
            event["object"]["size"].as_str().unwrap().to_string(),
        );
    }
    assert_eq!(sizes["r0"], "100");
    assert_eq!(sizes["r1"], "200");
}

#[tokio::test]
async fn dropping_watch_connection_releases_the_scenery() {
    let fx = fixture(0).await;

    let mut lines = get_stream(&fx.router, "/?offset=0&limit=2&watch=true").await;
    let _ = lines.next().await;
    assert_eq!(
        fx.dio.live_table_scenery_count(),
        1,
        "watch holds a scenery"
    );

    drop(lines);
    eventually("scenery released after disconnect", || {
        fx.dio.live_table_scenery_count() == 0
    })
    .await;
}

#[tokio::test]
async fn watch_detail_streams_record_changes() {
    let fx = fixture(1024).await;

    let mut lines = get_stream(&fx.router, "/r0?watch=true").await;
    let added = lines.next().await;
    assert_eq!(added["type"], "ADDED");
    assert_eq!(added["object"]["size"], "100", "watch opens hydrated");

    // An external change lands in the cache → the watch pushes MODIFIED.
    fx.dio
        .patched(
            "r0",
            record(&[("id", "r0"), ("modified", "t2"), ("size", "101")]),
        )
        .await
        .expect("patched");
    let modified = lines.next().await;
    assert_eq!(modified["type"], "MODIFIED");
    assert_eq!(modified["object"]["modified"], "t2");
    assert_eq!(modified["object"]["size"], "101");
}
