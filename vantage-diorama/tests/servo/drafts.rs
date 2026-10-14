//! New-record drafts: the id each `IdStrategy` binds, the id a driver
//! settles on, and a narrowed Dio filling and confining the insert.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, FlashKind, IdStrategy, Lens};
use vantage_vista::{Column, Condition, FilterOp, Vista, VistaMetadata, mocks::MockShell};

use crate::support::text;
use crate::{dio_over, int, product_shell, routed_dio, saving_writer};

#[tokio::test]
async fn uuid_strategy_mints_identity_up_front() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo_new(IdStrategy::Uuid);

    let id = servo.id().expect("identity exists before the first save");
    // Identity is the binding's, not the draft's: an untouched form is
    // clean and saving it fires nothing.
    assert_eq!(servo.get("id"), None, "the draft carries no id field");
    assert!(!servo.is_dirty(), "a fresh Uuid servo is clean");
    assert!(
        servo.flash().await?.is_none(),
        "an untouched form must not insert a blank record"
    );

    servo.set("name", text("Croissant"));
    let first = servo.flash().await?.expect("insert fires");
    assert_eq!(first.kind(), &FlashKind::Insert);
    assert_eq!(first.id(), Some(id.as_str()));
    assert_eq!(
        first.patch().get("id"),
        Some(&text(&id)),
        "the insert record picks the id column up at flash time"
    );

    // Continue editing the created record: same id, now a patch.
    servo.set("price", int(4));
    let second = servo.flash().await?.expect("follow-up fires");
    assert_eq!(second.kind(), &FlashKind::Patch, "no duplicate insert");
    assert_eq!(second.id(), Some(id.as_str()));
    assert_eq!(
        dio.master().get_value(&id).await?.unwrap().get("price"),
        Some(&int(4)),
    );
    Ok(())
}

#[tokio::test]
async fn uuid_create_retry_reuses_the_id() -> Result<()> {
    let shell = product_shell();
    let reject = Arc::new(AtomicBool::new(true));
    let dio = routed_dio(&shell, saving_writer(reject.clone())).await?;
    let servo = dio.servo_new(IdStrategy::Uuid);
    let id = servo.id().expect("minted at creation");

    servo.set("name", text("Croissant"));
    assert!(servo.flash().await.is_err(), "first create rejected");
    assert_eq!(servo.id().as_deref(), Some(id.as_str()), "id unchanged");
    assert!(servo.baseline().is_none(), "still a draft of a new record");

    reject.store(false, Ordering::SeqCst);
    let flash = servo.flash().await?.expect("retry fires");
    assert_eq!(flash.kind(), &FlashKind::Insert);
    assert_eq!(flash.id(), Some(id.as_str()), "same identity — idempotent");
    Ok(())
}

/// A driver is free to keep a new record under an id of its own. The
/// staged row must move onto that id — one record upstream is one row
/// here, not the staged row plus the refreshed one.
#[tokio::test]
async fn a_create_settles_on_the_id_the_driver_stored() -> Result<()> {
    let shell = MockShell::new().with_id_prefix("products:");
    let dio = dio_over(&shell).await?;
    let servo = dio.servo_new(IdStrategy::Uuid);
    let minted = servo.id().expect("minted at creation");

    servo.set("name", text("Croissant"));
    let flash = servo.flash().await?.expect("insert fires");

    let settled = format!("products:{minted}");
    assert_eq!(flash.id(), Some(settled.as_str()));
    assert_eq!(servo.id().as_deref(), Some(settled.as_str()));
    assert!(
        dio.cached().get_value(&minted).await?.is_none(),
        "the minted key is retired"
    );
    assert!(
        dio.cached().get_value(&settled).await?.is_some(),
        "the row lives under the id the driver reported"
    );
    assert_eq!(shell.len(), 1, "one record upstream");
    assert!(!servo.is_dirty(), "converged clean on the created row");
    Ok(())
}

/// The settled id comes out of the master's id column, whatever that
/// column is named. A master keyed by `sku` reports where it stored the
/// record in `sku`, so a re-key that only looked at a literal `id` field
/// would leave the row on the minted key.
#[tokio::test]
async fn a_create_settles_on_a_renamed_id_column() -> Result<()> {
    let shell = MockShell::new().with_id_prefix("products:");
    let metadata = VistaMetadata::new()
        .with_column(Column::new("sku", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_id_column("sku");
    let vista = Vista::new("products", shell.clone().with_metadata(metadata));
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    let dio = lens.dio(vista).build().await?;

    let servo = dio.servo_new(IdStrategy::Uuid);
    let minted = servo.id().expect("minted at creation");
    servo.set("name", text("Croissant"));
    let flash = servo.flash().await?.expect("insert fires");

    let settled = format!("products:{minted}");
    assert_eq!(flash.id(), Some(settled.as_str()));
    assert_eq!(servo.id().as_deref(), Some(settled.as_str()));
    assert!(
        dio.cached().get_value(&minted).await?.is_none(),
        "the minted key is retired"
    );
    let row = dio
        .cached()
        .get_value(&settled)
        .await?
        .expect("the row lives under the id the driver reported");
    assert_eq!(
        row.get("sku"),
        Some(&text(&settled)),
        "the master reports the id in its own id column"
    );
    Ok(())
}

/// The returning insert reports the same id the by-id insert stores
/// under. A driver that qualifies its keys must qualify this one too —
/// otherwise an `Auto` create binds, and seeds the cache with, an id no
/// read can resolve.
#[tokio::test]
async fn an_auto_create_binds_the_id_the_driver_qualified() -> Result<()> {
    let shell = MockShell::new().with_id_prefix("products:");
    let dio = dio_over(&shell).await?;
    let servo = dio.servo_new(IdStrategy::Auto);

    servo.set("name", text("Croissant"));
    let flash = servo.flash().await?.expect("returning insert fires");

    let id = servo.id().expect("bound to the created row");
    assert_eq!(
        shell.record_ids(),
        vec![id.clone()],
        "the id the store used"
    );
    assert_eq!(flash.id(), Some(id.as_str()));
    assert!(
        dio.master().get_value(&id).await?.is_some(),
        "the bound id resolves upstream"
    );
    assert!(
        dio.cached().get_value(&id).await?.is_some(),
        "cache seeded under that same id"
    );
    Ok(())
}

#[tokio::test]
async fn auto_strategy_binds_the_backend_id() -> Result<()> {
    let shell = product_shell();
    let dio = dio_over(&shell).await?;
    let servo = dio.servo_new(IdStrategy::Auto);

    assert!(
        servo.id().is_none(),
        "no identity until the backend assigns"
    );
    servo.set("name", text("Croissant"));
    let flash = servo.flash().await?.expect("returning insert fires");

    let id = servo.id().expect("bound to the created row");
    assert_eq!(flash.id(), Some(id.as_str()));
    assert!(
        dio.cached().get_value(&id).await?.is_some(),
        "cache seeded with the created row"
    );
    assert!(!servo.is_dirty(), "converged clean on the created row");

    // Further edits patch the same id.
    servo.set("price", int(2));
    let second = servo.flash().await?.expect("follow-up fires");
    assert_eq!(second.kind(), &FlashKind::Patch);
    assert_eq!(second.id(), Some(id.as_str()));
    Ok(())
}

/// A new record through a narrowed Dio takes the narrowing's equality as a
/// filled value, and one that contradicts it is refused, never stored.
#[tokio::test]
async fn servo_new_on_a_derived_dio_fills_and_confines_the_insert() -> Result<()> {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_column(Column::new("team", "String"))
        .with_id_column("id");
    let shell = MockShell::new().with_metadata(metadata);
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    let parent = lens
        .dio(Vista::new("members", shell.clone()))
        .build()
        .await?;
    let dio = parent
        .derive()
        .with_condition(Condition::new("team", FilterOp::Eq, "red"))
        .build()
        .await?;

    let servo = dio.servo_new(IdStrategy::Uuid);
    let id = servo.id().expect("minted at creation");
    servo.set("name", text("Ann"));
    servo.flash().await?.expect("insert fires");
    let stored = parent
        .master()
        .get_value(&id)
        .await?
        .expect("the master holds the row");
    assert_eq!(
        stored.get("team"),
        Some(&text("red")),
        "filled from the narrowing"
    );

    let blue = dio.servo_new(IdStrategy::Uuid);
    blue.set("name", text("Bob"));
    blue.set("team", text("blue"));
    let err = blue.flash().await.expect_err("outside the narrowing");
    assert!(err.is_conflict(), "{err}");
    assert_eq!(shell.len(), 1, "the master holds one row");
    Ok(())
}
