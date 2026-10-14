//! `flash()` freezes the error signal: a patch of only the dirty fields, an
//! insert from `servo_new`, a delete.

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{FlashKind, IdStrategy, ServoStatus};

use crate::support::text;
use crate::{dio_over, int, product_shell};

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
    let servo = dio.servo_new(IdStrategy::FromRecord);

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
        dio.cached().get_value("p1").await?.is_none(),
        "cache row gone"
    );
    Ok(())
}
