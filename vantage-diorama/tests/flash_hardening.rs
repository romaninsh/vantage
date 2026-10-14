//! Hardening of the flash pipeline:
//!
//! - **Effective write capabilities** — `dio.write_capabilities()` is the
//!   one gate UI chrome asks. Master caps by default; a Lens `writer` makes
//!   the Dio writable regardless of the master (a read-only CSV becomes
//!   editable, changes landing wherever the writer sends them).
//! - **Reconcile-while-pending** — a refresh carrying a master snapshot that
//!   predates an in-flight flash must not clobber it: `dio.reconcile()` skips
//!   rows with a flash in flight, and a confirmed flash re-asserts its fields
//!   over any stale write that raced it.
//! - **One write at a time** — the write queue runs a Dio's write-throughs
//!   in order, never two at once.
//! - **Drain, not drop** — flashes already in the queue keep the pipeline
//!   alive: dropping every external handle, and every waiting caller, still
//!   lands them.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(feature = "test-util")]
use std::time::Duration;

use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashOutcome, Lens, LoaderCx, ServoStatus};
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

mod support;

use support::flash::{cached, seeded_dio, staged};
use support::{record, text};

fn metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_column(Column::new("status", "String"))
        .with_id_column("id")
}

/// A master that can be read but not written — the mock analogue of a CSV
/// file or a third-party read-only API.
fn read_only_shell() -> MockShell {
    MockShell::new()
        .with_capabilities(VistaCapabilities::new())
        .with_record(
            "c1",
            record(&[("id", "c1"), ("name", "Ada"), ("status", "lead")]),
        )
}

fn vista_over(shell: &MockShell, name: &str) -> Vista {
    Vista::new(name, shell.clone().with_metadata(metadata()))
}

fn memory_lens() -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .build()
        .expect("lens")
}

// ---- effective write capabilities ------------------------------------------

#[tokio::test]
async fn write_capabilities_default_to_the_master() -> Result<()> {
    let lens = memory_lens();

    let read_only = lens
        .dio(vista_over(&read_only_shell(), "csv"))
        .build()
        .await?;
    let caps = read_only.write_capabilities();
    assert!(!caps.can_insert && !caps.can_update && !caps.can_delete);

    let writable = lens
        .dio(vista_over(&MockShell::new(), "db"))
        .build()
        .await?;
    let caps = writable.write_capabilities();
    assert!(caps.can_insert && caps.can_update && caps.can_delete);
    Ok(())
}

#[tokio::test]
async fn a_writer_makes_a_read_only_master_writable() -> Result<()> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(|_cx, _flash| async move { Ok(FlashOutcome::applied()) })
        .build()?;
    let dio = lens
        .dio(vista_over(&read_only_shell(), "csv"))
        .build()
        .await?;

    let caps = dio.write_capabilities();
    assert!(
        caps.can_insert && caps.can_update && caps.can_delete,
        "the writer takes the writes; the master's own caps no longer gate editing"
    );
    Ok(())
}

// ---- routed writes: read-only master, changes land elsewhere ----------------

#[tokio::test]
async fn routed_flash_lands_in_the_log_vista_and_never_touches_the_master() -> Result<()> {
    let master_shell = read_only_shell();
    let log_shell = MockShell::new();
    let log = Arc::new(vista_over(&log_shell, "audit-log"));

    let route_log = log.clone();
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(move |_cx, flash| {
            let log = route_log.clone();
            async move {
                // The routing ergonomic: bind the change to the log vista and
                // save the merged record there.
                flash.active_record(log.as_ref())?.save().await?;
                Ok(FlashOutcome::applied())
            }
        })
        .build()?;
    let dio = seeded_dio(&lens, vista_over(&master_shell, "csv")).await?;

    let servo = dio.servo("c1").await?;
    servo.set("status", text("won"));
    servo.flash().await?.expect("dirty servo flashes");
    assert!(matches!(servo.status(), ServoStatus::Tracking));

    // The change landed in the log vista — the full merged record.
    let landed = log.get_value("c1").await?.expect("routed row landed");
    assert_eq!(landed.get("status"), Some(&text("won")));
    assert_eq!(landed.get("name"), Some(&text("Ada")));

    // The cache keeps showing the edit (until a future reconcile says
    // otherwise) — the grid the user is looking at reflects their change.
    let row = cached(&dio, "c1").await?.unwrap();
    assert_eq!(row.get("status"), Some(&text("won")));

    // The read-only master was never written.
    let master_row = dio.master().get_value("c1").await?.unwrap();
    assert_eq!(master_row.get("status"), Some(&text("lead")));
    Ok(())
}

// ---- reconcile-while-pending ------------------------------------------------

/// A Lens whose writer waits for a permit from `gate`, pinning the pending
/// window open. The writer hands its `LoaderCx` to `seen`.
fn gated_lens(gate: Arc<Semaphore>, seen: Arc<Mutex<Option<LoaderCx>>>) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .writer(move |cx, _flash| {
            let gate = gate.clone();
            *seen.lock().unwrap() = Some(cx);
            async move {
                gate.acquire().await.expect("gate open").forget();
                Ok(FlashOutcome::applied())
            }
        })
        .build()
        .expect("lens")
}

fn items() -> MockShell {
    MockShell::new()
        .with_record(
            "a",
            record(&[("id", "a"), ("name", "one"), ("status", "open")]),
        )
        .with_record("b", record(&[("id", "b"), ("name", "two")]))
}

/// Seed `items()` under `lens` and fire a patch renaming "a" to "edited".
/// Returns once the gated writer holds that patch in the pending window.
async fn edit_a_in_flight(
    lens: &Arc<Lens>,
) -> Result<(Dio, tokio::task::JoinHandle<Result<FlashOutcome>>)> {
    let dio = seeded_dio(lens, vista_over(&items(), "items")).await?;
    let mut bus = dio.subscribe_events();
    let flight = tokio::spawn({
        let dio = dio.clone();
        async move {
            dio.flash(ChangeFlash::partial("a", record(&[("name", "edited")])))
                .await
        }
    });
    staged(&mut bus, &["a"]).await;
    Ok((dio, flight))
}

#[tokio::test]
async fn stale_reconcile_skips_rows_with_a_flash_in_flight() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let lens = gated_lens(gate.clone(), Arc::default());
    let (dio, flight) = edit_a_in_flight(&lens).await?;

    // A reconcile arrives carrying a master snapshot that predates the
    // write: stale for "a", fresh for "b".
    let snapshot = [
        ("a".to_string(), record(&[("id", "a"), ("name", "one")])),
        (
            "b".to_string(),
            record(&[("id", "b"), ("name", "two-updated")]),
        ),
    ];
    dio.reconcile(snapshot.into_iter().collect()).await?;

    let a = cached(&dio, "a").await?.unwrap();
    assert_eq!(
        a.get("name"),
        Some(&text("edited")),
        "the in-flight row must not be clobbered by the stale snapshot"
    );
    let b = cached(&dio, "b").await?.unwrap();
    assert_eq!(
        b.get("name"),
        Some(&text("two-updated")),
        "rows with no flash in flight reconcile normally"
    );

    // Release the write; after it confirms, reconciles apply again.
    gate.add_permits(1);
    flight.await.unwrap()?;
    let later = [(
        "a".to_string(),
        record(&[("id", "a"), ("name", "reconciled-later")]),
    )];
    dio.reconcile(later.into_iter().collect()).await?;
    let a = cached(&dio, "a").await?.unwrap();
    assert_eq!(a.get("name"), Some(&text("reconciled-later")));
    assert!(
        cached(&dio, "b").await?.is_none(),
        "absent from the snapshot"
    );
    Ok(())
}

#[tokio::test]
async fn confirmed_flash_reasserts_its_fields_over_a_raw_stale_write() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let seen = Arc::new(Mutex::new(None));
    let lens = gated_lens(gate.clone(), seen.clone());
    let (dio, flight) = edit_a_in_flight(&lens).await?;

    // A writer that ignores the pending guard clobbers the row mid-flight
    // with a stale snapshot (and a fresh other-field value).
    let cx = seen.lock().unwrap().clone().expect("the writer ran");
    let stale = record(&[("id", "a"), ("name", "one"), ("status", "closed")]);
    cx.cache().insert_value("a", &stale).await?;

    gate.add_permits(1);
    flight.await.unwrap()?;

    // The confirmed flash re-asserts the fields it wrote; fields it never
    // touched keep whatever arrived meanwhile.
    let a = cached(&dio, "a").await?.unwrap();
    assert_eq!(
        a.get("name"),
        Some(&text("edited")),
        "confirmed field re-asserted"
    );
    assert_eq!(
        a.get("status"),
        Some(&text("closed")),
        "untouched field not rolled back"
    );
    Ok(())
}

// ---- one write at a time ----------------------------------------------------

/// Two flashes raced on one Dio: the second write-through starts only once
/// the first has finished, and they land in the order they were queued.
#[tokio::test]
async fn concurrent_flashes_never_overlap_their_write_throughs() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let order = Arc::new(Mutex::new(Vec::new()));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let (gate, running, most, order) =
                (gate.clone(), running.clone(), most.clone(), order.clone());
            move |_cx, flash| {
                let (gate, running, most, order) =
                    (gate.clone(), running.clone(), most.clone(), order.clone());
                async move {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    gate.acquire().await.expect("gate open").forget();
                    order.lock().unwrap().push(flash.id().unwrap().to_string());
                    running.fetch_sub(1, Ordering::SeqCst);
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = seeded_dio(&lens, vista_over(&items(), "items")).await?;
    let mut bus = dio.subscribe_events();

    let first = tokio::spawn({
        let dio = dio.clone();
        async move {
            dio.flash(ChangeFlash::partial("a", record(&[("name", "x")])))
                .await
        }
    });
    staged(&mut bus, &["a"]).await;
    let second = tokio::spawn({
        let dio = dio.clone();
        async move {
            dio.flash(ChangeFlash::partial("b", record(&[("name", "y")])))
                .await
        }
    });
    staged(&mut bus, &["b"]).await;

    gate.add_permits(2);
    first.await.unwrap()?;
    second.await.unwrap()?;
    assert_eq!(most.load(Ordering::SeqCst), 1, "write-throughs overlapped");
    assert_eq!(*order.lock().unwrap(), ["a", "b"]);
    Ok(())
}

// ---- drain, not drop --------------------------------------------------------

/// Queued flashes land after every handle drops, and after their callers
/// stop waiting: the worker then exits on its own.
#[cfg(feature = "test-util")]
#[tokio::test]
async fn queued_flashes_survive_dropping_every_handle() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let shell = MockShell::new();
    let log = Arc::new(vista_over(&shell, "tasks"));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let (gate, log) = (gate.clone(), log.clone());
            move |_cx, flash| {
                let (gate, log) = (gate.clone(), log.clone());
                async move {
                    gate.acquire().await.expect("gate open").forget();
                    flash.active_record(log.as_ref())?.save().await?;
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = lens
        .dio(vista_over(&MockShell::new(), "tasks"))
        .build()
        .await?;
    let worker = dio.take_write_worker_handle().await.expect("worker handle");
    let mut bus = dio.subscribe_events();

    let callers: Vec<_> = ["t1", "t2"]
        .into_iter()
        .map(|id| {
            let dio = dio.clone();
            tokio::spawn(
                async move { dio.flash(ChangeFlash::insert(id, record(&[("id", id)]))).await },
            )
        })
        .collect();
    staged(&mut bus, &["t1", "t2"]).await;
    for caller in &callers {
        caller.abort();
    }
    drop((dio, bus, lens));
    gate.add_permits(2);

    // The queued flashes keep the pipeline alive until they land; the
    // worker then exits cleanly on its own.
    tokio::time::timeout(Duration::from_secs(5), worker)
        .await
        .expect("worker exited after draining")
        .expect("worker task completed");
    assert_eq!(shell.len(), 2, "both queued writes landed");
    Ok(())
}
