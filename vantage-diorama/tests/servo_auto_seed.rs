//! An `IdStrategy::Auto` create whose cache seed fails after the master
//! insert succeeded: the servo still binds the new id, so the next flash
//! patches that row instead of inserting a second one.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Cache, FlashKind, IdStrategy, Lens};
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

use support::spy_cache::{SpyCache, SpyTable};

fn product_vista(shell: &MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new("products", shell.clone().with_metadata(metadata))
}

#[tokio::test]
async fn a_failed_cache_seed_still_binds_the_created_id() -> Result<()> {
    let table = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .build()?;
    let shell = MockShell::new();
    let dio = lens.dio(product_vista(&shell)).build().await?;
    let servo = dio.servo_new(IdStrategy::Auto);

    table.fail_inserts.store(true, Ordering::SeqCst);
    servo.set("name", CborValue::Text("Croissant".into()));
    servo.flash().await?.expect("returning insert fires");
    let id = servo.id().expect("bound despite the failed seed");
    assert_eq!(shell.record_ids(), vec![id.clone()]);

    table.fail_inserts.store(false, Ordering::SeqCst);
    servo.set("name", CborValue::Text("Pain".into()));
    let second = servo.flash().await?.expect("follow-up fires");
    assert_eq!(second.kind(), &FlashKind::Patch);
    assert_eq!(second.id(), Some(id.as_str()));
    assert_eq!(shell.record_ids(), vec![id], "no second insert");
    Ok(())
}
