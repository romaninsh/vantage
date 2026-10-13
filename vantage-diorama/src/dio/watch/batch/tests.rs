use std::sync::Arc;

use async_trait::async_trait;
use vantage_core::error;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaChange, VistaChangeStream, VistaMetadata};

use super::*;
use crate::dio::watch::admit::Admit;
use crate::lens::{Cache, CacheBackend, CacheStatus, CacheTable, Lens, MemoryCacheTable};

type Rec = Record<CborValue>;

fn row(note: &str) -> Rec {
    [("note".to_string(), CborValue::Text(note.into()))]
        .into_iter()
        .collect()
}

fn inserted(id: &str, note: &str) -> VistaChange {
    VistaChange::Inserted {
        id: id.into(),
        value: row(note),
    }
}

fn ready(rest: Vec<Result<VistaChange>>) -> VistaChangeStream {
    Box::pin(futures::stream::iter(rest))
}

async fn dio_over(cache: Cache) -> Dio {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Lens::builder()
        .cache(cache)
        .build()
        .expect("lens")
        .dio(Vista::new("row", MockShell::new().with_metadata(metadata)))
        .build()
        .await
        .expect("dio")
}

async fn dio() -> Dio {
    dio_over(Cache::memory()).await
}

fn label(event: DioEvent) -> String {
    match event {
        DioEvent::RecordInserted { id } => format!("inserted {id}"),
        DioEvent::RecordChanged { id } => format!("changed {id}"),
        DioEvent::RecordRemoved { id } => format!("removed {id}"),
        DioEvent::Refreshing => "refreshing".into(),
        DioEvent::DatasetChanged => "dataset changed".into(),
        other => format!("{other:?}"),
    }
}

fn drain(bus: &mut tokio::sync::broadcast::Receiver<DioEvent>) -> Vec<String> {
    std::iter::from_fn(|| bus.try_recv().ok().map(label)).collect()
}

/// Apply `changes` as one ready pass: the first, then the rest as a stream
/// that has them all ready and then ends.
async fn apply_all(dio: &Dio, admit: &Admit, changes: Vec<VistaChange>) -> Result<()> {
    let mut changes = changes.into_iter();
    let first = changes.next().expect("at least one change");
    let mut stream = ready(changes.map(Ok).collect());
    apply_ready(&Arc::downgrade(&dio.inner), first, &mut stream, admit)
        .await
        .0
}

/// One unfiltered pass over `stream`, starting with an insert of `a`.
async fn pass(dio: &Dio, stream: &mut VistaChangeStream) -> (Result<()>, Option<Tail>) {
    let weak = Arc::downgrade(&dio.inner);
    apply_ready(&weak, inserted("a", "1"), stream, &Admit::All).await
}

/// Everything ready is applied in the pass; the stream's end is reported
/// after it.
#[tokio::test]
async fn a_ready_burst_is_one_pass_ending_with_the_stream() {
    let dio = dio().await;
    let mut stream = ready(vec![Ok(inserted("b", "1")), Ok(inserted("c", "1"))]);
    let (applied, tail) = pass(&dio, &mut stream).await;
    applied.unwrap();
    assert!(matches!(tail, Some(Tail::Ended)));
    assert_eq!(dio.inner.cache.count().await.unwrap(), 3);
}

#[tokio::test]
async fn a_stream_with_nothing_ready_applies_the_first_change() {
    let dio = dio().await;
    let mut stream: VistaChangeStream = Box::pin(futures::stream::pending());
    let (applied, tail) = pass(&dio, &mut stream).await;
    applied.unwrap();
    assert!(tail.is_none());
    assert_eq!(dio.inner.cache.count().await.unwrap(), 1);
}

/// A stream that never runs dry doesn't hold the Dio: once its last handle
/// drops, the pass ends.
#[tokio::test]
async fn a_never_dry_stream_ends_its_pass_when_the_dio_drops() {
    let dio = dio().await;
    let weak = Arc::downgrade(&dio.inner);
    let mut holder = Some(dio);
    let endless = futures::stream::iter(0..).map(move |i| {
        if i == 100 {
            drop(holder.take());
        }
        Ok(inserted(&i.to_string(), "1"))
    });
    let mut stream: VistaChangeStream = Box::pin(endless);
    let (_, tail) = apply_ready(&weak, inserted("a", "1"), &mut stream, &Admit::All).await;
    assert!(matches!(tail, Some(Tail::Dropped)));
}

#[tokio::test]
async fn a_stream_error_ends_the_pass_keeping_what_came_before() {
    let dio = dio().await;
    let mut stream = ready(vec![
        Ok(inserted("b", "1")),
        Err(error!("stream broke")),
        Ok(inserted("c", "1")),
    ]);
    let (applied, tail) = pass(&dio, &mut stream).await;
    applied.unwrap();
    assert!(matches!(tail, Some(Tail::Failed(_))));
    let cache = &dio.inner.cache;
    assert_eq!(cache.count().await.unwrap(), 2);
    assert_eq!(cache.get_value("c").await.unwrap(), None);
}

/// A pass leaves the cache and the bus as one-at-a-time application would:
/// a later row for an id wins, and a delete lands after the upserts before it.
#[tokio::test]
async fn a_pass_applies_in_order() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        VistaChange::Updated {
            id: "a".into(),
            value: row("2"),
        },
        inserted("b", "1"),
        VistaChange::Deleted { id: "b".into() },
        inserted("c", "1"),
    ];
    apply_all(&dio, &Admit::All, changes).await.unwrap();

    let cache = &dio.inner.cache;
    assert_eq!(cache.get_value("a").await.unwrap(), Some(row("2")));
    assert_eq!(cache.get_value("b").await.unwrap(), None);
    assert_eq!(cache.get_value("c").await.unwrap(), Some(row("1")));
    assert_eq!(
        drain(&mut bus),
        [
            "inserted a",
            "changed a",
            "inserted b",
            "removed b",
            "inserted c"
        ]
    );
}

/// `Invalidated` flushes the run before it and keeps its place in the order.
#[tokio::test]
async fn invalidated_flushes_the_run_first() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        VistaChange::Invalidated,
        inserted("b", "1"),
        inserted("c", "1"),
    ];
    apply_all(&dio, &Admit::All, changes).await.unwrap();
    assert_eq!(
        drain(&mut bus),
        [
            "inserted a",
            "refreshing",
            "dataset changed",
            "inserted b",
            "inserted c"
        ]
    );
}

/// Changes the follow can't judge cost one re-read for the whole pass, made
/// once the ready changes are in.
#[tokio::test]
async fn unjudgeable_changes_cost_one_reread_after_the_pass() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        VistaChange::Deleted { id: "c".into() },
        inserted("b", "1"),
    ];
    apply_all(&dio, &Admit::Reread, changes).await.unwrap();
    assert_eq!(drain(&mut bus), ["refreshing", "dataset changed"]);
}

/// A long run is committed in pieces of half the bus, yielding after each,
/// so a subscriber on the same thread reads every event without lagging.
#[tokio::test]
async fn a_long_run_is_committed_in_pieces_that_fit_the_bus() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let n = EVENT_BUS_CAPACITY * 3;
    let reader = tokio::spawn(async move {
        let mut seen = 0;
        while seen < n {
            match bus.recv().await {
                Ok(_) => seen += 1,
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(seen)
    });
    let changes = (0..n).map(|i| inserted(&format!("{i:04}"), "1"));
    apply_all(&dio, &Admit::All, changes.collect())
        .await
        .unwrap();
    assert_eq!(reader.await.unwrap(), Ok(n));
    assert_eq!(dio.inner.cache.count().await.unwrap(), n as i64);
}

/// A cache whose batched write always fails and whose single-row writes fail
/// for the id `bad`.
struct FlakyTable(MemoryCacheTable);

#[async_trait]
impl CacheTable for FlakyTable {
    async fn list_values(&self) -> Result<IndexMap<String, Rec>> {
        self.0.list_values().await
    }
    async fn get_value(&self, id: &str) -> Result<Option<Rec>> {
        self.0.get_value(id).await
    }
    async fn insert_value(&self, id: &str, record: &Rec) -> Result<()> {
        if id == "bad" {
            return Err(error!("cache write failed", id = id));
        }
        self.0.insert_value(id, record).await
    }
    async fn insert_values(&self, _rows: IndexMap<String, Rec>) -> Result<()> {
        Err(error!("batched cache write failed"))
    }
    async fn delete_value(&self, id: &str) -> Result<()> {
        self.0.delete_value(id).await
    }
    async fn clear(&self) -> Result<()> {
        self.0.clear().await
    }
    async fn count(&self) -> Result<i64> {
        self.0.count().await
    }
    async fn meta_total(&self) -> Result<Option<u64>> {
        self.0.meta_total().await
    }
    async fn set_meta_total(&self, total: u64) -> Result<()> {
        self.0.set_meta_total(total).await
    }
    async fn meta_refreshed_at(&self) -> Result<Option<std::time::SystemTime>> {
        self.0.meta_refreshed_at().await
    }
    async fn set_meta_refreshed_at(&self, at: std::time::SystemTime) -> Result<()> {
        self.0.set_meta_refreshed_at(at).await
    }
    async fn get_value_with_status(&self, id: &str) -> Result<Option<(Rec, CacheStatus)>> {
        self.0.get_value_with_status(id).await
    }
    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Rec,
        status: CacheStatus,
    ) -> Result<()> {
        if id == "bad" {
            return Err(error!("cache write failed", id = id));
        }
        self.0.insert_value_with_status(id, record, status).await
    }
    async fn list_values_with_status(&self) -> Result<IndexMap<String, (Rec, CacheStatus)>> {
        self.0.list_values_with_status().await
    }
}

/// Serves its one table under every name.
struct FlakyCache(Arc<FlakyTable>);

#[async_trait]
impl CacheBackend for FlakyCache {
    async fn open_table(&self, _name: &str) -> Result<Arc<dyn CacheTable>> {
        Ok(self.0.clone() as Arc<dyn CacheTable>)
    }
    async fn close(&self) {}
    async fn list_tables(&self) -> Result<Vec<String>> {
        Err(error!("FlakyCache does not enumerate tables"))
    }
    async fn drop_table(&self, name: &str) -> Result<()> {
        Err(error!("FlakyCache does not drop tables", table = name))
    }
    fn name(&self) -> &'static str {
        "flaky"
    }
}

/// A failed transaction falls back to row-by-row writes: only the row that
/// can't be written is lost, the changes after it still apply, and the
/// first error is still returned.
#[tokio::test]
async fn a_failed_commit_loses_only_the_rows_that_fail() {
    let table = Arc::new(FlakyTable(MemoryCacheTable::default()));
    table.0.insert_value("old", &row("1")).await.unwrap();
    let dio = dio_over(Cache::backend(Arc::new(FlakyCache(table)))).await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        inserted("bad", "1"),
        VistaChange::Deleted { id: "old".into() },
        inserted("c", "1"),
    ];
    let result = apply_all(&dio, &Admit::All, changes).await;
    assert!(result.is_err());

    let cache = &dio.inner.cache;
    assert_eq!(cache.get_value("a").await.unwrap(), Some(row("1")));
    assert_eq!(cache.get_value("bad").await.unwrap(), None);
    assert_eq!(cache.get_value("old").await.unwrap(), None);
    assert_eq!(cache.get_value("c").await.unwrap(), Some(row("1")));
    assert_eq!(drain(&mut bus), ["inserted a", "removed old", "inserted c"]);
}
