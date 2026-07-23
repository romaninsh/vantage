//! `Servo` — the editing companion as a servo loop over a record.
//!
//! `data` is the commanded setpoint, `baseline` the measured upstream state,
//! and the dirty set is the error signal (`data ≠ baseline`, computed by
//! diff). Untouched fields run in continuous tracking — upstream changes
//! update them live and they stay clean; touched fields lock and hold.
//! `flash()` freezes the error signal into an immutable [`ChangeFlash`]
//! carrying only the changed fields.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Dio, FlashKind, Generation, Lens, ServoStatus};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

/// Product master: one seeded row `p1` and a live handle to mutate the
/// store out-of-band (the "another process wrote to the backend" lever).
fn product_shell() -> MockShell {
    MockShell::new().with_record(
        "p1",
        rec(&[
            ("id", text("p1")),
            ("name", text("Coffee")),
            ("price", int(3)),
            ("stock", int(10)),
        ]),
    )
}

fn product_vista(shell: &MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_column(Column::new("price", "int"))
        .with_column(Column::new("stock", "int"))
        .with_id_column("id");
    Vista::new("products", Box::new(shell.clone().with_metadata(metadata)))
}

/// Dio over an in-memory cache seeded from the master's current rows.
async fn dio_over(shell: &MockShell) -> Result<Dio> {
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("build lens"));
    let dio = lens.make_dio(product_vista(shell)).await?;
    for (id, r) in dio.master().list_values().await? {
        dio.cache().insert_value(&id, &r).await?;
    }
    Ok(dio)
}

async fn wait_for_gen(rx: &mut tokio::sync::watch::Receiver<Generation>, current: u64) -> u64 {
    tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > current {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.expect("watch channel closed");
        }
    })
    .await
    .expect("timed out waiting for generation bump")
}

// ---- setpoints, error signal, revert ---------------------------------------

#[tokio::test]
async fn set_commands_a_setpoint_and_raises_the_error_signal() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    assert!(!servo.is_dirty(), "freshly opened servo has zero error");

    servo.set("name", text("Tea"));
    assert!(servo.dirty("name"));
    assert!(!servo.dirty("price"), "untouched field stays in tracking");
    let error = servo.error();
    assert_eq!(error.get("name"), Some(&text("Tea")));
    assert_eq!(error.len(), 1, "error signal carries only commanded fields");
    Ok(())
}

#[tokio::test]
async fn set_bumps_generation_for_subscribers() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();

    let g0 = u64::from(*gen_rx.borrow_and_update());
    servo.set("name", text("Tea"));
    wait_for_gen(&mut gen_rx, g0).await;
    Ok(())
}

#[tokio::test]
async fn revert_releases_the_setpoint_back_to_tracking() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    servo.set("name", text("Tea"));
    servo.revert("name");

    assert!(!servo.is_dirty());
    assert_eq!(
        servo.get("name"),
        Some(text("Coffee")),
        "value back to baseline"
    );
    Ok(())
}

#[tokio::test]
async fn setting_the_baseline_value_is_zero_error() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    // Commanding the measured value: error is zero by definition.
    servo.set("name", text("Coffee"));
    assert!(!servo.is_dirty());
    Ok(())
}

// ---- continuous tracking while editing -------------------------------------

#[tokio::test]
async fn clean_fields_track_upstream_while_a_field_is_locked() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();

    servo.set("name", text("Tea")); // lock one field
    let g = u64::from(*gen_rx.borrow_and_update());

    // Upstream reports a new state: stock moved, name unchanged upstream.
    dio.patched(
        "p1",
        rec(&[
            ("id", text("p1")),
            ("name", text("Coffee")),
            ("price", int(3)),
            ("stock", int(99)),
        ]),
    )
    .await?;
    wait_for_gen(&mut gen_rx, g).await;

    assert_eq!(
        servo.get("stock"),
        Some(int(99)),
        "tracking field followed upstream"
    );
    assert!(!servo.dirty("stock"), "...and stayed clean");
    assert_eq!(
        servo.get("name"),
        Some(text("Tea")),
        "locked field held its setpoint"
    );
    assert!(servo.dirty("name"));
    Ok(())
}

#[tokio::test]
async fn upstream_convergence_zeroes_the_error() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();

    servo.set("name", text("Tea"));
    let g = u64::from(*gen_rx.borrow_and_update());

    // Upstream independently arrives at the commanded value.
    dio.patched(
        "p1",
        rec(&[
            ("id", text("p1")),
            ("name", text("Tea")),
            ("price", int(3)),
            ("stock", int(10)),
        ]),
    )
    .await?;
    wait_for_gen(&mut gen_rx, g).await;

    assert!(
        !servo.is_dirty(),
        "error hit zero from the measurement side"
    );
    assert_eq!(servo.get("name"), Some(text("Tea")));
    Ok(())
}

// ---- flash: freeze the error signal ----------------------------------------

#[tokio::test]
async fn flash_patches_only_the_dirty_fields() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    // The master moves on its own AFTER our cache/baseline snapshot: a
    // whole-record write would clobber this with the stale stock=10.
    shell.set_field("p1", "stock", int(77));

    servo.set("name", text("Tea"));
    let flash = servo.flash().await?.expect("dirty servo produces a flash");

    assert_eq!(flash.kind(), &FlashKind::Patch);
    assert_eq!(flash.patch().len(), 1, "only the error signal travels");
    assert_eq!(flash.patch().get("name"), Some(&text("Tea")));

    let master_row = dio.master().get_value("p1").await?.unwrap();
    assert_eq!(master_row.get("name"), Some(&text("Tea")), "patch landed");
    assert_eq!(
        master_row.get("stock"),
        Some(&int(77)),
        "untouched fields never travel — the master's own value survives"
    );
    Ok(())
}

#[tokio::test]
async fn flash_settles_the_servo_clean() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    servo.set("name", text("Tea"));
    servo.flash().await?;

    assert!(!servo.is_dirty(), "after actuation the error is zero");
    assert_eq!(servo.get("name"), Some(text("Tea")));
    assert!(matches!(servo.status(), ServoStatus::Tracking));
    Ok(())
}

#[tokio::test]
async fn flash_with_zero_error_is_a_no_op() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    assert!(
        servo.flash().await?.is_none(),
        "nothing dirty, nothing fired"
    );
    Ok(())
}

#[tokio::test]
async fn a_fired_flash_is_frozen_against_later_edits() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    servo.set("name", text("Tea"));
    let flash = servo.flash().await?.unwrap();

    // Keep editing after the shutter fired; the emitted flash must not move.
    servo.set("name", text("Espresso"));
    servo.set("price", int(9));

    assert_eq!(flash.patch().len(), 1);
    assert_eq!(flash.patch().get("name"), Some(&text("Tea")));
    assert_eq!(
        flash.before().and_then(|b| b.get("name").cloned()),
        Some(text("Coffee")),
        "the pre-image is the baseline at fire time"
    );
    Ok(())
}

// ---- insert / delete --------------------------------------------------------

#[tokio::test]
async fn servo_new_flashes_an_insert() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo_new();

    servo.set("id", text("p2"));
    servo.set("name", text("Croissant"));
    servo.set("price", int(2));
    let flash = servo.flash().await?.expect("new record produces a flash");

    assert_eq!(flash.kind(), &FlashKind::Insert);
    assert_eq!(flash.id(), Some("p2"));

    let master_row = dio.master().get_value("p2").await?.unwrap();
    assert_eq!(master_row.get("name"), Some(&text("Croissant")));
    assert!(!servo.is_dirty(), "insert settles the servo clean");
    Ok(())
}

#[tokio::test]
async fn servo_delete_flashes_a_delete() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo("p1").await?;

    let flash = servo.delete().await?;

    assert_eq!(flash.kind(), &FlashKind::Delete);
    assert!(
        dio.master().get_value("p1").await?.is_none(),
        "master row gone"
    );
    assert!(
        dio.cache().get_value("p1").await?.is_none(),
        "cache row gone"
    );
    Ok(())
}

// ---- failure surfaces, never silently --------------------------------------

#[tokio::test]
async fn rejected_flash_reverts_and_reports_failed() -> Result<()> {
    let shell = product_shell();
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_flash(|_dio, _flash| async move {
                Err(vantage_core::error!("route rejected the flash"))
            })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(product_vista(&shell)).await?;
    for (id, r) in dio.master().list_values().await? {
        dio.cache().insert_value(&id, &r).await?;
    }
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();

    servo.set("name", text("Tea"));
    let g = u64::from(*gen_rx.borrow_and_update());
    let result = servo.flash().await;
    assert!(result.is_err(), "the rejection surfaces to the caller");

    // The optimistic stage was rolled back; the servo re-absorbs the
    // restored pre-image and reports the failure.
    wait_for_gen(&mut gen_rx, g).await;
    assert!(matches!(servo.status(), ServoStatus::Failed(_)));
    assert_eq!(
        dio.cache().get_value("p1").await?.unwrap().get("name"),
        Some(&text("Coffee")),
        "cache pre-image restored"
    );
    Ok(())
}

#[tokio::test]
async fn foreign_delete_failure_leaves_servo_tracking() -> Result<()> {
    // A servo on a record is a bystander to a failed toolbar delete of
    // that record: the revert restores the row, but the failure belongs
    // to the delete's issuer (the confirm dialog) — not the edit form.
    let shell = product_shell();
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_flash(|_dio, flash| async move {
                if flash.kind() == &FlashKind::Delete {
                    Err(vantage_core::error!("FOREIGN KEY constraint failed"))
                } else {
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(product_vista(&shell)).await?;
    for (id, r) in dio.master().list_values().await? {
        dio.cache().insert_value(&id, &r).await?;
    }
    let servo = dio.servo("p1").await?;
    let mut events = dio.subscribe_events();

    let result = dio.flash_delete("p1").await;
    assert!(
        result.is_err(),
        "the rejection surfaces to the delete caller"
    );

    // Wait until the revert has been broadcast, then let the servo's
    // absorb task run.
    loop {
        match events.recv().await {
            Ok(vantage_diorama::DioEvent::WriteReverted { id, .. }) if id == "p1" => break,
            Ok(_) => {}
            Err(e) => panic!("event bus closed early: {e}"),
        }
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        matches!(servo.status(), ServoStatus::Tracking),
        "a failed delete must not read as the form's save failure, got {:?}",
        servo.status()
    );
    assert_eq!(
        dio.cache().get_value("p1").await?.unwrap().get("name"),
        Some(&text("Coffee")),
        "cache pre-image restored"
    );
    Ok(())
}
