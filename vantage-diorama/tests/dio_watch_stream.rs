//! A live follow over a scripted change stream: a stream that errors or
//! ends, a narrowing the follow can't judge, and a cache that refuses
//! writes. In each case the follow re-reads the set, so the cache can't
//! drift from the master silently.

mod support;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use futures::StreamExt;
use tokio::sync::{Semaphore, broadcast};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, Dio, DioEvent, Lens};
use vantage_types::Record;
use vantage_vista::{
    Capability, Condition, FilterOp, NativeCondition, TableShell, Vista, VistaCapabilities,
    VistaChange, VistaChangeStream, forward_table_shell, mocks::MockShell,
};

use support::spy_cache::{SpyCache, SpyTable};
use support::{eventually, memory_lens, meta, page_master, record};

const WITHIN: Duration = Duration::from_secs(5);

type Script = Vec<Result<VistaChange>>;

/// How the streams a [`Scripted`] master hands out behave.
#[derive(Clone)]
struct Streams {
    /// One script per subscription, in order; once they run out, a stream
    /// yields nothing.
    scripts: Arc<Mutex<VecDeque<Script>>>,
    /// A stream ends after its script instead of staying open.
    end: bool,
    /// The first stream waits for one permit before its script.
    gate: Option<Arc<Semaphore>>,
    opened: Arc<AtomicUsize>,
    /// While set, subscribing fails: the backend is unreachable.
    refusing: Arc<AtomicBool>,
}

/// A watchable master pushing [`Streams`]. It takes any native condition,
/// and ignores it when listing.
#[derive(Clone)]
struct Scripted {
    inner: Box<dyn TableShell>,
    capabilities: VistaCapabilities,
    streams: Streams,
}

forward_table_shell!(Scripted, inner, {
    async fn watch(&self) -> Result<VistaChangeStream> {
        if self.streams.refusing.load(Ordering::SeqCst) {
            return Err(vantage_core::error!("the backend is unreachable"));
        }
        let first = self.streams.opened.fetch_add(1, Ordering::SeqCst) == 0;
        let script = self
            .streams
            .scripts
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default();
        let gate = self.streams.gate.clone().filter(|_| first);
        let changes = futures::stream::once(async move {
            if let Some(gate) = gate {
                gate.acquire().await.expect("gate open").forget();
            }
            futures::stream::iter(script)
        })
        .flatten();
        if self.streams.end {
            Ok(Box::pin(changes))
        } else {
            Ok(Box::pin(changes.chain(futures::stream::pending())))
        }
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn add_native_condition(&mut self, _condition: NativeCondition) -> Result<()> {
        Ok(())
    }
});

fn row(id: &str) -> Record<CborValue> {
    record(&[("id", id), ("name", id)])
}

fn inserted(id: &str) -> Result<VistaChange> {
    Ok(VistaChange::Inserted {
        id: id.into(),
        value: row(id),
    })
}

fn streams(scripts: Vec<Script>) -> Streams {
    Streams {
        scripts: Arc::new(Mutex::new(scripts.into())),
        end: false,
        gate: None,
        opened: Arc::default(),
        refusing: Arc::default(),
    }
}

/// A master named `items` holding `ids`, pushing `streams`.
fn master(ids: &[&str], streams: Streams) -> Vista {
    let shell = ids.iter().fold(
        MockShell::new().with_metadata(meta(&["id", "name"])),
        |shell, id| shell.with_record(*id, row(id)),
    );
    let capabilities = shell
        .capabilities()
        .with(Capability::Subscribe)
        .with(Capability::NativeConditions);
    Vista::new("items", shell).map_shell(|inner| Scripted {
        inner,
        capabilities,
        streams,
    })
}

fn memory() -> Arc<Lens> {
    Lens::builder().cache(Cache::memory()).build().unwrap()
}

async fn cached_ids(dio: &Dio) -> Vec<String> {
    let mut ids: Vec<String> = dio
        .cached()
        .list_values()
        .await
        .map(|rows| rows.into_keys().collect())
        .unwrap_or_default();
    ids.sort();
    ids
}

/// Wait until `dio` caches exactly `want`.
async fn holds_exactly(dio: &Dio, want: &[&str]) -> bool {
    tokio::time::timeout(WITHIN, async {
        while cached_ids(dio).await != want {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

/// Every event `bus` has queued, as `inserted <id>` / `refresh failed`.
fn labels(bus: &mut broadcast::Receiver<DioEvent>) -> Vec<String> {
    std::iter::from_fn(|| bus.try_recv().ok())
        .filter_map(|event| match event {
            DioEvent::RecordInserted { id } => Some(format!("inserted {id}")),
            DioEvent::RefreshFailed { .. } => Some("refresh failed".into()),
            _ => None,
        })
        .collect()
}

/// The changes before a stream error apply; then the follow resubscribes and
/// re-reads, so the cache ends as the master's set.
#[tokio::test]
async fn a_stream_error_resubscribes_and_re_reads_the_set() -> Result<()> {
    let script = vec![
        inserted("a"),
        inserted("z"),
        Err(vantage_core::error!("the connection dropped")),
    ];
    let streams = streams(vec![script]);
    let opened = streams.opened.clone();
    let dio = memory()
        .dio(master(&["a", "b", "c"], streams))
        .live()
        .build()
        .await?;

    assert!(
        holds_exactly(&dio, &["a", "b", "c"]).await,
        "cached {:?}",
        cached_ids(&dio).await
    );
    assert!(opened.load(Ordering::SeqCst) >= 2, "it resubscribed");
    Ok(())
}

/// A stream that ends is a dropped subscription: the follow resubscribes and
/// re-reads.
#[tokio::test]
async fn a_stream_that_ends_resubscribes_and_re_reads_the_set() -> Result<()> {
    let mut streams = streams(vec![vec![inserted("z")]]);
    streams.end = true;
    let opened = streams.opened.clone();
    let dio = memory().dio(master(&["a"], streams)).live().build().await?;

    assert!(
        holds_exactly(&dio, &["a"]).await,
        "cached {:?}",
        cached_ids(&dio).await
    );
    assert!(opened.load(Ordering::SeqCst) >= 2, "it resubscribed");
    Ok(())
}

/// A native condition can't be judged in memory, so no pushed row is
/// trusted: a push re-reads the set instead.
#[tokio::test]
async fn under_a_native_condition_a_push_re_reads_the_set() -> Result<()> {
    let burst: Script = (0..40).map(|i| inserted(&format!("x{i:02}"))).collect();
    let narrowed = master(&["a", "b"], streams(vec![burst]))
        .with_native_condition(NativeCondition::new((), "is_paying = true"))?;
    let dio = memory().dio(narrowed).live().build().await?;

    assert!(
        holds_exactly(&dio, &["a", "b"]).await,
        "cached {:?}",
        cached_ids(&dio).await
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        cached_ids(&dio).await,
        ["a", "b"],
        "no pushed row was cached"
    );
    Ok(())
}

/// A master that compares `client` as a record link, as SurrealDB does: a
/// filter's bare key narrows to the rows holding that link.
#[derive(Clone)]
struct Linking {
    inner: Box<dyn TableShell>,
}

fn link(key: &str) -> CborValue {
    CborValue::Tag(
        8,
        Box::new(CborValue::Array(vec!["client".into(), key.into()])),
    )
}

fn linked(condition: &Condition) -> Condition {
    let mut typed = condition.clone();
    if let (CborValue::Text(key), "client") = (&condition.value, condition.column.as_str()) {
        typed.value = link(key);
    }
    typed
}

forward_table_shell!(Linking, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.inner.add_condition(&linked(condition))
    }
    fn comparable_condition(&self, condition: &Condition) -> Condition {
        linked(condition)
    }
});

fn loan(id: &str, client: &str, n: i64) -> Record<CborValue> {
    [
        ("id".to_string(), CborValue::from(id)),
        ("client".to_string(), link(client)),
        ("n".to_string(), CborValue::from(n)),
    ]
    .into_iter()
    .collect()
}

/// A pushed row holds the typed value its backend compared the filter's
/// text as, so the follow judges it by the typed condition and keeps it.
#[tokio::test]
async fn a_typed_filter_keeps_its_row_through_a_live_update() -> Result<()> {
    let update = vec![Ok(VistaChange::Updated {
        id: "a".into(),
        value: loan("a", "abc", 2),
    })];
    let shell = MockShell::new()
        .with_metadata(meta(&["id", "client", "n"]))
        .with_record("a", loan("a", "abc", 1))
        .with_record("b", loan("b", "xyz", 1));
    let capabilities = shell.capabilities().with(Capability::Subscribe);
    let streams = streams(vec![update]);
    let master = Vista::new("loans", shell)
        .map_shell(|inner| Scripted {
            inner,
            capabilities,
            streams,
        })
        .map_shell(|inner| Linking { inner })
        .with_condition("client", FilterOp::Eq, "abc")?;
    let dio = memory().dio(master).live().build().await?;

    let updated = tokio::time::timeout(WITHIN, async {
        loop {
            let a = dio.cached().get_value("a").await.ok().flatten();
            if a.is_some_and(|row| row.get("n") == Some(&CborValue::from(2))) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(updated.is_ok(), "cached {:?}", cached_ids(&dio).await);
    assert_eq!(cached_ids(&dio).await, ["a"]);
    Ok(())
}

/// A live Dio over a spied cache whose master holds `a` and `b` and, once
/// `gate` opens, pushes both together.
async fn spied(gate: &Arc<Semaphore>) -> Result<(Dio, Arc<SpyTable>)> {
    let mut streams = streams(vec![vec![inserted("a"), inserted("b")]]);
    streams.gate = Some(gate.clone());
    let table = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .build()?;
    let dio = lens
        .dio(master(&["a", "b"], streams))
        .live()
        .build()
        .await?;
    Ok((dio, table))
}

/// Pushed rows land and are announced as `apply_change` would land them one
/// by one, even when the cache refuses to write them together.
#[tokio::test]
async fn pushed_rows_land_and_are_announced_when_a_batched_write_fails() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let (dio, table) = spied(&gate).await?;
    table.fail_batches.store(true, Ordering::SeqCst);
    let mut bus = dio.subscribe_events();
    gate.add_permits(1);

    assert!(
        holds_exactly(&dio, &["a", "b"]).await,
        "cached {:?}",
        cached_ids(&dio).await
    );
    let seen = labels(&mut bus);
    assert!(seen.contains(&"inserted a".to_string()), "{seen:?}");
    assert!(seen.contains(&"inserted b".to_string()), "{seen:?}");
    Ok(())
}

/// A pushed row the cache won't write isn't announced. The follow re-reads
/// the set; while that fails too it publishes `RefreshFailed` and retries,
/// and once the cache takes writes again the row lands.
#[tokio::test]
async fn a_row_the_cache_refuses_is_not_announced_and_the_re_read_is_retried() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let (dio, table) = spied(&gate).await?;
    table.fail_batches.store(true, Ordering::SeqCst);
    table.fail_ids.lock().unwrap().push("b".into());
    let mut bus = dio.subscribe_events();
    gate.add_permits(1);

    let mut seen = Vec::new();
    let failed = tokio::time::timeout(WITHIN, async {
        loop {
            seen.extend(labels(&mut bus));
            if seen.iter().any(|l| l == "refresh failed") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(failed.is_ok(), "no RefreshFailed: {seen:?}");
    assert!(seen.contains(&"inserted a".to_string()), "{seen:?}");
    assert!(!seen.contains(&"inserted b".to_string()), "{seen:?}");
    assert_eq!(cached_ids(&dio).await, ["a"]);

    table.fail_batches.store(false, Ordering::SeqCst);
    table.fail_ids.lock().unwrap().clear();
    assert!(
        holds_exactly(&dio, &["a", "b"]).await,
        "the retried re-read never landed b: {:?}",
        cached_ids(&dio).await
    );
    Ok(())
}

/// The backend can't be subscribed to when the Dio is built. With
/// `retry_subscribe` the build goes ahead and the follow keeps trying; once
/// it subscribes it re-reads, so the open view shows a row written while
/// nothing was followed.
#[tokio::test]
async fn scenery_recovers_after_a_failed_first_subscribe() -> Result<()> {
    let shell = MockShell::new()
        .with_metadata(meta(&["id", "name"]))
        .with_capabilities(VistaCapabilities::new().with(Capability::Window))
        .with_record("a", row("a"));
    let streams = streams(vec![]);
    streams.refusing.store(true, Ordering::SeqCst);
    let (refusing, opened) = (streams.refusing.clone(), streams.opened.clone());
    let capabilities = shell.capabilities().with(Capability::Subscribe);
    let vista = Vista::new("items", shell.clone()).map_shell(|inner| Scripted {
        inner,
        capabilities,
        streams,
    });
    let dio = memory_lens(page_master())
        .dio(vista)
        .live()
        .retry_subscribe()
        .build()
        .await?;
    let scenery = dio.table_scenery().open().await?;
    scenery.set_viewport(0..3, 0..0);
    let names = || -> Vec<String> {
        (0..scenery.row_count())
            .filter_map(|i| scenery.row(i))
            .filter_map(|r| match r.record.get("name") {
                Some(CborValue::Text(name)) => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    assert!(
        eventually(WITHIN, || names() == ["a"]).await,
        "{:?}",
        names()
    );

    shell.set_record("b", row("b"));
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(names(), ["a"], "nothing re-reads while it can't subscribe");
    refusing.store(false, Ordering::SeqCst);

    assert!(
        eventually(WITHIN, || names() == ["a", "b"]).await,
        "{:?}",
        names()
    );
    assert_eq!(opened.load(Ordering::SeqCst), 1, "subscribed once it could");
    Ok(())
}

/// A reconnect re-reads a paged view's windows: a row deleted while the
/// follow was away leaves the view and the cache, the kept rows stay.
#[tokio::test]
async fn a_reconnect_drops_the_rows_a_paged_view_holds_that_are_gone() -> Result<()> {
    let shell = ["a", "b", "c"].iter().fold(
        MockShell::new()
            .with_metadata(meta(&["id", "name"]))
            .with_capabilities(VistaCapabilities::new().with(Capability::Window)),
        |shell, id| shell.with_record(*id, row(id)),
    );
    let gate = Arc::new(Semaphore::new(0));
    let mut streams = streams(vec![vec![Err(vantage_core::error!(
        "the connection dropped"
    ))]]);
    streams.gate = Some(gate.clone());
    let capabilities = shell.capabilities().with(Capability::Subscribe);
    let vista = Vista::new("items", shell.clone()).map_shell(|inner| Scripted {
        inner,
        capabilities,
        streams,
    });
    let dio = memory_lens(page_master()).dio(vista).live().build().await?;
    let scenery = dio.table_scenery().open().await?;
    scenery.set_viewport(0..3, 0..0);
    let names = || -> Vec<String> {
        (0..scenery.row_count())
            .filter_map(|i| scenery.row(i))
            .filter_map(|r| match r.record.get("name") {
                Some(CborValue::Text(name)) => Some(name.clone()),
                _ => None,
            })
            .collect()
    };
    assert!(eventually(WITHIN, || names().len() == 3).await);

    shell.remove_record("b");
    gate.add_permits(1);

    assert!(
        holds_exactly(&dio, &["a", "c"]).await,
        "cached {:?}",
        cached_ids(&dio).await
    );
    assert!(
        eventually(WITHIN, || names() == ["a", "c"]).await,
        "{:?}",
        names()
    );
    Ok(())
}
