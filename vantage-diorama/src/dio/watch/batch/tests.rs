use std::sync::Arc;

use async_trait::async_trait;
use vantage_core::error;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Vista, VistaChange, VistaChangeStream};

use super::*;
use crate::lens::{CacheBackend, CacheTable, Lens, MemoryCacheTable};

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

async fn dio_over(lens: Lens) -> Dio {
    Arc::new(lens)
        .make_dio(Vista::new("row", MockShell::new()))
        .await
        .expect("dio")
}

async fn dio() -> Dio {
    dio_over(Lens::new().cache_in_memory().build().expect("lens")).await
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

/// Everything ready joins the batch; the stream's end is reported after it.
#[tokio::test]
async fn a_ready_burst_is_one_batch_ending_with_the_stream() {
    let mut stream = ready(vec![Ok(inserted("b", "1")), Ok(inserted("c", "1"))]);
    let (batch, tail) = ready_batch(inserted("a", "1"), &mut stream);
    assert_eq!(batch.len(), 3);
    assert!(matches!(tail, Some(Tail::Ended)));
}

#[tokio::test]
async fn a_stream_with_nothing_ready_gives_a_batch_of_one() {
    let mut stream: VistaChangeStream = Box::pin(futures::stream::pending());
    let (batch, tail) = ready_batch(inserted("a", "1"), &mut stream);
    assert_eq!(batch.len(), 1);
    assert!(tail.is_none());
}

#[tokio::test]
async fn a_stream_error_ends_the_batch_keeping_what_came_before() {
    let mut stream = ready(vec![
        Ok(inserted("b", "1")),
        Err(error!("stream broke")),
        Ok(inserted("c", "1")),
    ]);
    let (batch, tail) = ready_batch(inserted("a", "1"), &mut stream);
    assert_eq!(batch.len(), 2);
    assert!(matches!(tail, Some(Tail::Failed(_))));
}

#[tokio::test]
async fn a_batch_stops_at_its_cap() {
    let rest = (0..MAX_BATCH).map(|i| Ok(inserted(&i.to_string(), "1")));
    let mut stream = ready(rest.collect());
    let (batch, tail) = ready_batch(inserted("first", "1"), &mut stream);
    assert_eq!(batch.len(), MAX_BATCH);
    assert!(tail.is_none());
}

/// A batch leaves the cache and the bus as one-at-a-time application would:
/// a later row for an id wins, and a delete lands after the upserts before it.
#[tokio::test]
async fn a_batch_applies_in_order() {
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
    dio.apply_changes(changes.into_iter().map(Into::into).collect())
        .await
        .unwrap();

    let cache = dio.cache();
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

/// `Invalidated` and a notify-only change flush the run before them and keep
/// their place in the order.
#[tokio::test]
async fn invalidated_and_notify_only_changes_flush_the_run_first() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1").into(),
        ChangeEvent::Invalidated,
        inserted("b", "1").into(),
        ChangeEvent::Updated {
            id: "a".into(),
            new: None,
        },
        inserted("c", "1").into(),
    ];
    dio.apply_changes(changes).await.unwrap();
    assert_eq!(
        drain(&mut bus),
        [
            "inserted a",
            "refreshing",
            "dataset changed",
            "inserted b",
            "changed a",
            "inserted c"
        ]
    );
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
    let changes = (0..n).map(|i| inserted(&format!("{i:04}"), "1").into());
    dio.apply_changes(changes.collect()).await.unwrap();
    assert_eq!(reader.await.unwrap(), Ok(n));
    assert_eq!(dio.cache().count().await.unwrap(), n as i64);
}

/// A cache whose batched write always fails and whose single-row write fails
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
}

struct FlakyCache(Arc<FlakyTable>);

#[async_trait]
impl CacheBackend for FlakyCache {
    async fn open_table(&self, _name: &str) -> Result<Arc<dyn CacheTable>> {
        Ok(self.0.clone() as Arc<dyn CacheTable>)
    }
}

/// A failed transaction falls back to row-by-row writes: only the row that
/// can't be written is lost, the changes after it still apply, and the
/// first error is still returned.
#[tokio::test]
async fn a_failed_commit_loses_only_the_rows_that_fail() {
    let table = Arc::new(FlakyTable(MemoryCacheTable::default()));
    table.0.insert_value("old", &row("1")).await.unwrap();
    let lens = Lens::new()
        .cache_source(Arc::new(FlakyCache(table)))
        .build()
        .expect("lens");
    let dio = dio_over(lens).await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        inserted("bad", "1"),
        VistaChange::Deleted { id: "old".into() },
        inserted("c", "1"),
    ];
    let result = dio
        .apply_changes(changes.into_iter().map(Into::into).collect())
        .await;
    assert!(result.is_err());

    let cache = dio.cache();
    assert_eq!(cache.get_value("a").await.unwrap(), Some(row("1")));
    assert_eq!(cache.get_value("bad").await.unwrap(), None);
    assert_eq!(cache.get_value("old").await.unwrap(), None);
    assert_eq!(cache.get_value("c").await.unwrap(), Some(row("1")));
    assert_eq!(drain(&mut bus), ["inserted a", "removed old", "inserted c"]);
}
