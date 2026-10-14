//! Effective write capabilities, and writes a writer routes away from a
//! read-only master.

use std::sync::Arc;

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, FlashOutcome, Lens, ServoStatus};
use vantage_vista::mocks::MockShell;

use super::{memory_lens, read_only_shell, vista_over};
use crate::support::flash::{cached, seeded_dio};
use crate::support::text;

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
