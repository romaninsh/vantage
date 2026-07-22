//! Tier-2 hardening of the flash pipeline:
//!
//! - **Effective write capabilities** — `dio.write_capabilities()` is the
//!   one gate UI chrome asks. Master caps by default; an `on_flash` route
//!   makes the Dio writable regardless of the master (a read-only CSV
//!   becomes editable, changes landing wherever the route sends them).
//! - **Reconcile-while-pending** — a refresh pulling a master snapshot
//!   that predates an in-flight flash must not clobber it:
//!   `dio.reconcile_values()` skips rows with a flash in flight, and a
//!   confirmed flash re-asserts its fields over any stale write that
//!   raced it.
//! - **Drain, not drop** — flashes already in the queue keep the pipeline
//!   alive: dropping every external handle still lands them.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::Notify;
use vantage_core::Result;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{Lens, ServoStatus};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), text(v));
    }
    r
}

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
        .with_capabilities(VistaCapabilities {
            can_count: true,
            ..VistaCapabilities::default()
        })
        .with_record(
            "c1",
            rec(&[("id", "c1"), ("name", "Ada"), ("status", "lead")]),
        )
}

fn vista_over(shell: &MockShell, name: &str) -> Vista {
    Vista::new(name, Box::new(shell.clone().with_metadata(metadata())))
}

async fn seed_cache(dio: &vantage_diorama::Dio) -> Result<()> {
    for (id, r) in dio.master().list_values().await? {
        dio.cache().insert_value(&id, &r).await?;
    }
    Ok(())
}

// ---- effective write capabilities ------------------------------------------

#[tokio::test]
async fn write_capabilities_default_to_the_master() -> Result<()> {
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));

    let read_only = lens.make_dio(vista_over(&read_only_shell(), "csv")).await?;
    let caps = read_only.write_capabilities();
    assert!(!caps.can_insert && !caps.can_update && !caps.can_delete);

    let writable = lens.make_dio(vista_over(&MockShell::new(), "db")).await?;
    let caps = writable.write_capabilities();
    assert!(caps.can_insert && caps.can_update && caps.can_delete);
    Ok(())
}

#[tokio::test]
async fn a_flash_route_makes_a_read_only_master_writable() -> Result<()> {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_flash(|_dio, _flash| async move { Ok(()) })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(vista_over(&read_only_shell(), "csv")).await?;

    let caps = dio.write_capabilities();
    assert!(
        caps.can_insert && caps.can_update && caps.can_delete,
        "the route is the writer; the master's own caps no longer gate editing"
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
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_flash(move |_dio, flash| {
                let log = route_log.clone();
                async move {
                    // The routing ergonomic: bind the change to the log
                    // vista and save the merged record there.
                    flash.active_record(log.as_ref())?.save().await?;
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(vista_over(&master_shell, "csv")).await?;
    seed_cache(&dio).await?;

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
    let cached = dio.cache().get_value("c1").await?.unwrap();
    assert_eq!(cached.get("status"), Some(&text("won")));

    // The read-only master was never written.
    let master_row = dio.master().get_value("c1").await?.unwrap();
    assert_eq!(master_row.get("status"), Some(&text("lead")));
    Ok(())
}

// ---- reconcile-while-pending ------------------------------------------------

/// Lens whose route blocks on `gate`, pinning the pending window open.
fn gated_lens(gate: Arc<Notify>) -> Arc<Lens> {
    Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_flash(move |_dio, _flash| {
                let gate = gate.clone();
                async move {
                    gate.notified().await;
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    )
}

#[tokio::test]
async fn stale_reconcile_skips_rows_with_a_flash_in_flight() -> Result<()> {
    let gate = Arc::new(Notify::new());
    let lens = gated_lens(gate.clone());
    let shell = MockShell::new()
        .with_record("a", rec(&[("id", "a"), ("name", "one")]))
        .with_record("b", rec(&[("id", "b"), ("name", "two")]));
    let dio = lens.make_dio(vista_over(&shell, "items")).await?;
    seed_cache(&dio).await?;

    // Fire a patch; the gated route holds it in the pending window.
    let dio2 = dio.clone();
    let mut partial = Record::new();
    partial.insert("name".to_string(), text("edited"));
    let flight = tokio::spawn(async move { dio2.flash_patch("a", partial).await });

    // Wait until the optimistic stage is visible.
    for _ in 0..200 {
        if dio
            .cache()
            .get_value("a")
            .await?
            .and_then(|r| r.get("name").cloned())
            == Some(text("edited"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // A reconcile arrives carrying a master snapshot that predates the
    // write: stale for "a", fresh for "b".
    dio.reconcile_values(vec![
        ("a".to_string(), rec(&[("id", "a"), ("name", "one")])),
        (
            "b".to_string(),
            rec(&[("id", "b"), ("name", "two-updated")]),
        ),
    ])
    .await?;

    let a = dio.cache().get_value("a").await?.unwrap();
    assert_eq!(
        a.get("name"),
        Some(&text("edited")),
        "the in-flight row must not be clobbered by the stale snapshot"
    );
    let b = dio.cache().get_value("b").await?.unwrap();
    assert_eq!(
        b.get("name"),
        Some(&text("two-updated")),
        "rows with no flash in flight reconcile normally"
    );

    // Release the write; after it confirms, reconciles apply again.
    gate.notify_one();
    flight.await.unwrap()?;
    dio.reconcile_values(vec![(
        "a".to_string(),
        rec(&[("id", "a"), ("name", "reconciled-later")]),
    )])
    .await?;
    let a = dio.cache().get_value("a").await?.unwrap();
    assert_eq!(a.get("name"), Some(&text("reconciled-later")));
    Ok(())
}

#[tokio::test]
async fn confirmed_flash_reasserts_its_fields_over_a_raw_stale_write() -> Result<()> {
    let gate = Arc::new(Notify::new());
    let lens = gated_lens(gate.clone());
    let shell = MockShell::new().with_record(
        "a",
        rec(&[("id", "a"), ("name", "one"), ("status", "open")]),
    );
    let dio = lens.make_dio(vista_over(&shell, "items")).await?;
    seed_cache(&dio).await?;

    let dio2 = dio.clone();
    let mut partial = Record::new();
    partial.insert("name".to_string(), text("edited"));
    let flight = tokio::spawn(async move { dio2.flash_patch("a", partial).await });
    for _ in 0..200 {
        if dio
            .cache()
            .get_value("a")
            .await?
            .and_then(|r| r.get("name").cloned())
            == Some(text("edited"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    // A writer that doesn't go through reconcile_values clobbers the row
    // mid-flight with a stale snapshot (and a fresh other-field value).
    dio.cache()
        .insert_value(
            "a",
            &rec(&[("id", "a"), ("name", "one"), ("status", "closed")]),
        )
        .await?;

    gate.notify_one();
    flight.await.unwrap()?;

    // The confirmed flash re-asserts the fields it wrote; fields it never
    // touched keep whatever arrived meanwhile.
    let a = dio.cache().get_value("a").await?.unwrap();
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

// ---- drain, not drop --------------------------------------------------------

#[tokio::test]
async fn queued_flashes_survive_dropping_every_handle() -> Result<()> {
    let shell = MockShell::new();
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));
    let dio = lens.make_dio(vista_over(&shell, "tasks")).await?;
    let worker = dio.take_write_worker_handle().await.expect("worker handle");

    // Fire-and-forget enqueues through the facade, then drop everything.
    let facade = dio.vista();
    facade
        .insert_value("t1", &rec(&[("name", "first")]))
        .await?;
    facade
        .insert_value("t2", &rec(&[("name", "second")]))
        .await?;
    drop(facade);
    drop(dio);
    drop(lens);

    // The queued flashes keep the pipeline alive until they land; the
    // worker then exits cleanly on its own.
    tokio::time::timeout(Duration::from_secs(2), worker)
        .await
        .expect("worker exited after draining")
        .expect("worker task completed");

    assert_eq!(
        shell.len(),
        2,
        "both queued writes landed after every handle dropped"
    );
    Ok(())
}
