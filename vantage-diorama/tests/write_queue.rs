//! The write queue's edge cases: a caller cancelled mid-stage, a failure
//! under a flash stacked on the same row, a writer that writes its own Dio,
//! a writer that panics, a writer over a natively narrowed master, and the
//! settle event a two-pass view needs to follow membership.

mod support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::{Semaphore, broadcast};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{
    Cache, CacheTable, ChangeFlash, Dio, DioEvent, FlashOutcome, Lens, LoaderCx,
};
use vantage_types::Record;
use vantage_vista::{NativeCondition, TableShell, Vista, forward_table_shell, mocks::MockShell};

use support::spy_cache::{SpyCache, SpyTable};
use support::{meta, record};

fn items() -> MockShell {
    MockShell::new()
        .with_metadata(meta(&["id", "name"]))
        .with_record("a", record(&[("id", "a"), ("name", "one")]))
}

fn name_of(row: Option<Record<CborValue>>) -> Option<String> {
    match row?.get("name") {
        Some(CborValue::Text(name)) => Some(name.clone()),
        _ => None,
    }
}

/// A Dio over [`items`] on `lens`, its cache holding the master's rows.
async fn seeded(lens: &Arc<Lens>, master: Vista) -> Result<Dio> {
    let dio = lens.dio(master).build().await?;
    dio.reconcile(dio.master().list_values().await?).await?;
    Ok(dio)
}

fn rename(name: &str) -> ChangeFlash {
    ChangeFlash::partial("a", record(&[("name", name)]))
}

/// Wait until the bus announces a staged flash.
async fn staged(bus: &mut broadcast::Receiver<DioEvent>) {
    let wait = async { while !matches!(bus.recv().await, Ok(DioEvent::WritePending { .. })) {} };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("the flash was never staged");
}

/// A caller cancelled while its value is being staged: the flash never
/// reaches the queue, the staged value is put back, and nothing is written.
#[tokio::test]
async fn a_caller_cancelled_mid_stage_leaves_no_staged_value() -> Result<()> {
    let table = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .build()?;
    let shell = items();
    let dio = seeded(&lens, Vista::new("items", shell.clone())).await?;

    table.stall_next_insert.store(true, Ordering::SeqCst);
    let caller = tokio::spawn({
        let dio = dio.clone();
        async move { dio.flash(rename("edited")).await }
    });
    table.stalled.notified().await;
    assert_eq!(
        name_of(table.inner.get_value("a").await?).as_deref(),
        Some("edited")
    );

    let mut bus = dio.subscribe_events();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let restored = async {
        while !matches!(bus.recv().await, Ok(DioEvent::RecordChanged { id }) if id == "a") {}
    };
    tokio::time::timeout(Duration::from_secs(5), restored)
        .await
        .expect("the staged row was never restored");

    let row = table.inner.get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("one"), "pre-image restored");
    let upstream = shell.get_record("a");
    assert_eq!(
        name_of(upstream).as_deref(),
        Some("one"),
        "nothing was written"
    );
    Ok(())
}

/// The gate each held flash waits on, keyed by the id it writes.
type Gates = Arc<Mutex<Vec<(String, Arc<Semaphore>)>>>;

/// A writer that holds each flash on its own gate, then fails the one
/// renaming to `fail` and confirms the rest.
fn gated_writer(gates: Gates) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .writer(move |_cx, flash| {
            let name = match flash.patch().get("name") {
                Some(CborValue::Text(name)) => name.clone(),
                _ => String::new(),
            };
            let gate = gates
                .lock()
                .unwrap()
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, g)| g.clone());
            async move {
                if let Some(gate) = gate {
                    gate.acquire().await.expect("gate open").forget();
                }
                if name == "fail" {
                    return Err(vantage_core::error!("route rejected the flash"));
                }
                Ok(FlashOutcome::applied())
            }
        })
        .build()
        .expect("lens")
}

/// Two flashes stacked on one row. The first fails while the second is
/// staged over it: its rollback leaves the second's staged value alone, and
/// the second then settles.
#[tokio::test]
async fn a_failed_flash_under_a_stacked_one_keeps_the_later_value() -> Result<()> {
    let (first_gate, second_gate) = (Arc::new(Semaphore::new(0)), Arc::new(Semaphore::new(0)));
    let gates = vec![
        ("fail".to_string(), first_gate.clone()),
        ("kept".to_string(), second_gate.clone()),
    ];
    let lens = gated_writer(Arc::new(Mutex::new(gates)));
    let dio = seeded(&lens, Vista::new("items", items())).await?;
    let mut bus = dio.subscribe_events();

    let first = tokio::spawn({
        let dio = dio.clone();
        async move { dio.flash(rename("fail")).await }
    });
    staged(&mut bus).await;
    let second = tokio::spawn({
        let dio = dio.clone();
        async move { dio.flash(rename("kept")).await }
    });
    staged(&mut bus).await;

    first_gate.add_permits(1);
    assert!(first.await.unwrap().is_err());
    let row = dio.cached().get_value("a").await?;
    assert_eq!(
        name_of(row).as_deref(),
        Some("kept"),
        "the stacked value stands"
    );

    second_gate.add_permits(1);
    second.await.unwrap()?;
    let row = dio.cached().get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("kept"));
    Ok(())
}

/// A writer that flashes the Dio it is writing for gets a plain error naming
/// the Dio instead of waiting on itself forever.
#[tokio::test]
async fn a_writer_flashing_its_own_dio_is_refused() -> Result<()> {
    let own: Arc<OnceLock<Dio>> = Arc::default();
    let inner_error: Arc<Mutex<Option<String>>> = Arc::default();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let (own, inner_error) = (own.clone(), inner_error.clone());
            move |_cx, _flash| {
                let (own, inner_error) = (own.clone(), inner_error.clone());
                async move {
                    let dio = own.get().expect("dio set").clone();
                    let audit = ChangeFlash::insert("audit", record(&[("id", "audit")]));
                    if let Err(e) = dio.flash(audit).await {
                        *inner_error.lock().unwrap() = Some(e.to_string());
                    }
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = seeded(&lens, Vista::new("items", items())).await?;
    own.set(dio.clone()).ok();

    tokio::time::timeout(Duration::from_secs(5), dio.flash(rename("edited")))
        .await
        .expect("the write must not hang")?;
    let message = inner_error
        .lock()
        .unwrap()
        .clone()
        .expect("the inner flash failed");
    assert!(
        message.contains("cannot flash the Dio it is writing for"),
        "{message}"
    );
    assert!(message.contains("items"), "names the Dio: {message}");
    Ok(())
}

/// A writer that panics fails its flash, which rolls back; the worker lives
/// on and takes the next write.
#[tokio::test]
async fn a_panicking_writer_rolls_back_and_the_queue_survives() -> Result<()> {
    let panic_next = Arc::new(AtomicBool::new(true));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let panic_next = panic_next.clone();
            move |_cx: LoaderCx, _flash| {
                let panic_now = panic_next.swap(false, Ordering::SeqCst);
                async move {
                    assert!(!panic_now, "the writer panics on purpose");
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = seeded(&lens, Vista::new("items", items())).await?;

    let err = dio.flash(rename("lost")).await.unwrap_err();
    assert!(err.to_string().contains("panicked"), "{err}");
    let row = dio.cached().get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("one"), "rolled back");

    dio.flash(rename("kept")).await?;
    let row = dio.cached().get_value("a").await?;
    assert_eq!(
        name_of(row).as_deref(),
        Some("kept"),
        "the worker took the next write"
    );
    Ok(())
}

/// Takes any native condition.
#[derive(Clone)]
struct Native {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Native, inner, {
    fn add_native_condition(&mut self, _condition: NativeCondition) -> Result<()> {
        Ok(())
    }
});

/// Under a writer, which bypasses the master's own confinement, a native
/// condition can't be judged in memory, so every write is refused with an
/// error naming the clause and the Dio.
#[tokio::test]
async fn a_writer_over_a_native_narrowing_refuses_writes() -> Result<()> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(|_cx, _flash| async { Ok(FlashOutcome::applied()) })
        .build()?;
    let master = Vista::new("items", items())
        .map_shell(|inner| Native { inner })
        .with_native_condition(NativeCondition::new((), "is_paying = true"))?;
    let dio = lens.dio(master).build().await?;

    let err = dio.flash(rename("edited")).await.unwrap_err();
    let message = err.to_string();
    assert!(message.contains("native condition"), "{message}");
    assert!(
        message.contains("is_paying = true"),
        "names the clause: {message}"
    );
    assert!(message.contains("items"), "names the Dio: {message}");
    Ok(())
}

/// Rows of `view` whose `kind` is `kind`.
fn count_kind(view: &support::MockView, kind: &str) -> usize {
    (0..view.row_count())
        .filter(|&i| view.col_at(i, "kind").as_deref() == Some(kind))
        .count()
}

/// A two-pass (augmented) view keeps its own index of ids, so it follows a
/// write only through membership events: a confirmed facade delete drops the
/// row, and a confirmed insert of a new id shows it.
#[tokio::test]
async fn a_two_pass_view_follows_facade_deletes_and_inserts() -> Result<()> {
    use vantage_dataset::traits::WritableValueSet;

    let dio = support::bucket_dio().await;
    let view = support::MockView::open(&dio, 10).await;
    view.settle_until("listed", |v| v.row_count() == 3).await;

    dio.cached().delete("o1").await?;
    view.settle_until("o1 dropped", |v| {
        v.row_count() == 2 && count_kind(v, "red") == 1
    })
    .await;

    dio.cached()
        .insert_value("o4", &record(&[("id", "o4"), ("kind", "green")]))
        .await?;
    view.settle_until("o4 shown", |v| {
        v.row_count() == 3 && count_kind(v, "green") == 1
    })
    .await;
    Ok(())
}
