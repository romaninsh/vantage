//! A refused or failed flash surfaces, never silently: the draft survives,
//! the cache pre-image comes back, and a retry can land it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{
    ChangeFlash, DioEvent, FlashKind, FlashOutcome, FlashRejection, ServoStatus,
};
use vantage_vista::{Capability, Condition, FilterOp};

use crate::support::chunk::wait_for_gen;
use crate::support::text;
use crate::{dio_over, int, product_shell, routed_dio, saving_writer};

// ---- failure surfaces, never silently --------------------------------------

#[tokio::test]
async fn rejected_flash_reverts_and_reports_failed() -> Result<()> {
    let shell = product_shell();
    let dio = routed_dio(&shell, saving_writer(Arc::new(AtomicBool::new(true)))).await?;
    let servo = dio.servo("p1").await?;

    servo.set("name", text("Tea"));
    let result = servo.flash().await;
    assert!(result.is_err(), "the rejection surfaces to the caller");

    // The optimistic stage was rolled back and the pre-image absorbed —
    // but the servo is a draft: the setpoint survives the failure.
    assert!(matches!(servo.status(), ServoStatus::Failed(_)));
    assert_eq!(
        dio.cached().get_value("p1").await?.unwrap().get("name"),
        Some(&text("Coffee")),
        "cache pre-image restored"
    );
    assert_eq!(
        servo.get("name"),
        Some(text("Tea")),
        "the user's value survives the rejection"
    );
    assert!(servo.dirty("name"), "and still reads dirty");
    assert_eq!(
        servo.baseline().unwrap().get("name"),
        Some(&text("Coffee")),
        "baseline is the restored measurement"
    );
    Ok(())
}

#[tokio::test]
async fn draft_holds_through_pending() -> Result<()> {
    // While the write-through is in flight the servo must stay a draft:
    // setpoints locked, baseline unmoved, status Pending — the staged
    // optimistic cache value must not read as an upstream measurement.
    let shell = product_shell();
    let gate = Arc::new(tokio::sync::Notify::new());
    let release = gate.clone();
    let dio = routed_dio(&shell, move |_cx, _flash| {
        let gate = gate.clone();
        async move {
            gate.notified().await;
            Ok(FlashOutcome::applied())
        }
    })
    .await?;
    let servo = Arc::new(dio.servo("p1").await?);
    let mut bus = dio.subscribe_events();

    servo.set("name", text("Tea"));
    let in_flight = {
        let servo = servo.clone();
        tokio::spawn(async move { servo.flash().await })
    };
    // Wait for the flash to stage; its writer then blocks on the gate.
    let staged = async { while !matches!(bus.recv().await, Ok(DioEvent::WritePending { .. })) {} };
    tokio::time::timeout(Duration::from_secs(5), staged)
        .await
        .expect("the flash was never staged");

    assert!(matches!(servo.status(), ServoStatus::Pending));
    assert_eq!(servo.get("name"), Some(text("Tea")), "setpoint locked");
    assert!(servo.dirty("name"), "still dirty while pending");
    assert_eq!(
        servo.baseline().unwrap().get("name"),
        Some(&text("Coffee")),
        "baseline hasn't moved on hope"
    );

    release.notify_one();
    in_flight.await.expect("join")?;

    assert!(!servo.is_dirty(), "confirmation converged the draft clean");
    assert!(matches!(servo.status(), ServoStatus::Tracking));
    Ok(())
}

#[tokio::test]
async fn failed_save_retries_successfully() -> Result<()> {
    let shell = product_shell();
    let reject = Arc::new(AtomicBool::new(true));
    let dio = routed_dio(&shell, saving_writer(reject.clone())).await?;
    let servo = dio.servo("p1").await?;

    servo.set("name", text("Tea"));
    assert!(servo.flash().await.is_err());
    assert!(servo.dirty("name"), "draft survives the first failure");

    reject.store(false, Ordering::SeqCst);
    let flash = servo.flash().await?.expect("retry fires the held draft");
    assert_eq!(flash.patch().get("name"), Some(&text("Tea")));
    assert!(!servo.is_dirty(), "retry landed and converged clean");
    assert!(matches!(servo.status(), ServoStatus::Tracking));
    Ok(())
}

/// A servo on a row a derived Dio's residual leaves out starts with no
/// baseline, and its flash is refused before anything is written.
#[tokio::test]
async fn a_servo_outside_the_set_starts_empty_and_is_confined() -> Result<()> {
    let shell = product_shell().with_capabilities(
        [Capability::Insert, Capability::Update, Capability::Delete]
            .into_iter()
            .collect(),
    );
    let dio = dio_over(&shell).await?;
    let not_coffee = dio
        .derive()
        .with_condition(Condition::new("name", FilterOp::Ne, "Coffee"))
        .build()
        .await?;
    assert!(!not_coffee.residual().conditions.is_empty());

    let servo = not_coffee.servo("p1").await?;
    assert!(servo.baseline().is_none(), "p1 is outside the set");
    servo.set("price", int(9));
    let err = servo.flash().await.unwrap_err();
    assert!(err.is_conflict(), "{err}");
    assert!(matches!(servo.status(), ServoStatus::Failed(_)));
    assert_eq!(shell.get_record("p1").unwrap().get("price"), Some(&int(3)));
    Ok(())
}

#[tokio::test]
async fn rejection_field_errors_reach_the_status() -> Result<()> {
    let shell = product_shell();
    let dio = routed_dio(&shell, |_cx, _flash| async move {
        Err(FlashRejection::new("validation failed")
            .with_field("price", "must be positive")
            .into_error())
    })
    .await?;
    let servo = dio.servo("p1").await?;

    servo.set("price", int(-5));
    assert!(servo.flash().await.is_err());

    let ServoStatus::Failed(rejection) = servo.status() else {
        panic!("expected Failed, got {:?}", servo.status());
    };
    assert_eq!(rejection.message(), "validation failed");
    assert_eq!(rejection.error_for("price"), Some("must be positive"));
    assert!(
        servo.dirty("price"),
        "the named field still holds its draft"
    );
    Ok(())
}

#[tokio::test]
async fn foreign_delete_failure_leaves_servo_tracking() -> Result<()> {
    // A servo on a record is a bystander to a failed toolbar delete of
    // that record: the revert restores the row, but the failure belongs
    // to the delete's issuer (the confirm dialog) — not the edit form.
    let shell = product_shell();
    let dio = routed_dio(&shell, |_cx, flash| async move {
        if flash.kind() == &FlashKind::Delete {
            Err(vantage_core::error!("FOREIGN KEY constraint failed"))
        } else {
            Ok(FlashOutcome::applied())
        }
    })
    .await?;
    let servo = dio.servo("p1").await?;
    let mut gen_rx = servo.subscribe();
    let g = u64::from(*gen_rx.borrow_and_update());

    let result = dio.flash(ChangeFlash::delete("p1")).await;
    assert!(
        result.is_err(),
        "the rejection surfaces to the delete caller"
    );

    // The delete-kind WritePending no longer touches the servo, so the
    // only generation bump after the delete is the revert's absorb —
    // waiting for it proves the servo has seen the restored cache.
    wait_for_gen(&mut gen_rx, g).await;

    assert!(
        matches!(servo.status(), ServoStatus::Tracking),
        "a failed delete must not read as the form's save failure, got {:?}",
        servo.status()
    );
    assert_eq!(
        dio.cached().get_value("p1").await?.unwrap().get("name"),
        Some(&text("Coffee")),
        "cache pre-image restored"
    );
    Ok(())
}
