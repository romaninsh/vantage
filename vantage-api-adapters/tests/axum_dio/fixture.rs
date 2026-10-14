use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use ciborium::Value as CborValue;
use futures_util::StreamExt;
use tempfile::TempDir;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use vantage_api_adapters::axum_dio::DioRouter;
use vantage_diorama::{Augmentation, Cache, Detail, Dio, Fetch, Lens, MergeRule, Source, Tuning};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

pub fn record(pairs: &[(&str, &str)]) -> Record<CborValue> {
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

pub struct Fixture {
    pub dio: Dio,
    pub router: Router,
    /// The gated get-only detail source; its get log counts the fetches.
    pub detail: MockShell,
    pub gate: Arc<Semaphore>,
    _tmp: TempDir,
}

/// Dio over two master rows (id, modified) augmented with a `size` detail;
/// the cache is pre-seeded the way a server's startup sync would.
pub async fn fixture(open_permits: usize) -> Fixture {
    let tmp = TempDir::new().unwrap();

    let master_rows = [
        ("r0", record(&[("id", "r0"), ("modified", "t1")])),
        ("r1", record(&[("id", "r1"), ("modified", "t1")])),
    ];
    let mut master = MockShell::new();
    for (id, rec) in &master_rows {
        master = master.with_record(*id, rec.clone());
    }
    let master = Vista::new("files", master.with_metadata(meta(&["id", "modified"])));

    let gate = Arc::new(Semaphore::new(open_permits));
    let detail = MockShell::new()
        .with_metadata(meta(&["id", "size"]))
        .with_record("r0", record(&[("id", "r0"), ("size", "100")]))
        .with_record("r1", record(&[("id", "r1"), ("size", "200")]))
        .get_only()
        .gated(gate.clone());

    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .tuning(Tuning::default().with_viewport_debounce(Duration::from_millis(1)))
        .build()
        .expect("lens builds");
    let dio = lens
        .dio(master)
        .augment(vec![Augmentation::new(
            Detail::Fixed(Arc::new(Vista::new("sizes", detail.clone()))),
            Source::Id,
            Fetch::PerRow,
            MergeRule::lift(["size"]),
        )])
        .build()
        .await
        .expect("dio builds");

    // Seed the cache like a startup sync pump would.
    let seed: indexmap::IndexMap<String, Record<CborValue>> = master_rows
        .iter()
        .map(|(id, rec)| (id.to_string(), rec.clone()))
        .collect();
    dio.reconcile(seed).await.expect("seed cache");

    let router = DioRouter::new(dio.clone())
        .with_column("name", "id")
        .with_column("modified", "modified")
        .with_column("size", "size")
        .with_page_size(10)
        .into_router();

    Fixture {
        dio,
        router,
        detail,
        gate,
        _tmp: tmp,
    }
}

pub async fn get_json(router: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
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
pub async fn get_stream(router: &Router, uri: &str) -> LineReader {
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

pub struct LineReader {
    stream: std::pin::Pin<
        Box<dyn futures_util::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Send>,
    >,
    buffer: String,
}

impl LineReader {
    /// Next event line, parsed. Panics after two seconds of silence.
    pub async fn next(&mut self) -> serde_json::Value {
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
