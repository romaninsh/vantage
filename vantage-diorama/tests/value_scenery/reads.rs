//! What a value reads: hydrated rows, admitted rows, the cache's row count,
//! and a recovery after a failed read.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::aggregate::{Count, Sum};
use vantage_diorama::{Cache, ChangeEvent, Dio, Lens, ValueStatus};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, FilterOp, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell,
};

use super::{cbor_text, items_metadata, record, seeded_shell};
use crate::support::spy_cache::{SpyCache, SpyTable};
use crate::support::{
    augmented_dio, cond, copy_master, eventually, fixed_augmentation, memory_lens,
};

const WITHIN: Duration = Duration::from_secs(5);

fn int(n: i64) -> Option<CborValue> {
    Some(CborValue::Integer(n.into()))
}

/// The seeded items over a spied cache, copied whole by an eager start.
async fn spied_dio() -> Result<(Arc<SpyTable>, Dio)> {
    let table = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .loader(copy_master())
        .build()?;
    let dio = lens.dio(Vista::new("items", seeded_shell())).build().await?;
    Ok((table, dio))
}

/// Touch the bus without changing any row.
async fn touch(dio: &Dio) -> Result<()> {
    dio.apply_change(ChangeEvent::Updated {
        id: "nonexistent".into(),
        new: None,
    })
    .await
}

/// The eager copy caches the master rows without `size`; only the detail
/// source has it. A sum read straight off the cache would be zero.
#[tokio::test]
async fn a_value_over_an_augmented_column_hydrates_first() -> Result<()> {
    let size = |v: i64| -> Record<CborValue> {
        [("size".to_string(), CborValue::Integer(v.into()))]
            .into_iter()
            .collect()
    };
    let detail_meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("size", "i64"))
        .with_id_column("id");
    let detail = MockShell::new()
        .with_metadata(detail_meta)
        .with_record("a", size(3))
        .with_record("b", size(4))
        .with_record("c", size(5));
    // The augmentation keys the detail by the master row's `id` field.
    let keyed = |id: &str, name: &str, price: i64| {
        let mut r = record(name, price);
        r.insert("id".to_string(), cbor_text(id));
        r
    };
    let master = MockShell::new()
        .with_metadata(items_metadata())
        .with_record("a", keyed("a", "alpha", 10))
        .with_record("b", keyed("b", "beta", 20))
        .with_record("c", keyed("c", "gamma", 30));
    let dio = augmented_dio(
        &memory_lens(copy_master()),
        Vista::new("items", master),
        vec![fixed_augmentation(Vista::new("sizes", detail), &["size"])],
    )
    .await;

    let total = dio.value_scenery(Sum::of("size")).open().await?;
    assert_eq!(total.value(), int(12), "{:?}", total.status());
    Ok(())
}

/// A master without `FilterOps` refuses `price > 15`, so the derived Dio
/// holds it as a residual over a cache that still holds all three rows. A
/// narrowed count reads the admitted rows, not the cache's row count.
#[tokio::test]
async fn a_value_reads_only_rows_the_residual_admits() -> Result<()> {
    let caps: VistaCapabilities = [Capability::Order, Capability::Search]
        .into_iter()
        .collect();
    let parent = memory_lens(copy_master())
        .dio(Vista::new("items", seeded_shell().with_capabilities(caps)))
        .build()
        .await?;
    let dio = parent
        .derive()
        .with_query(&cond("price", FilterOp::Gt, 15))
        .build()
        .await?;
    assert_eq!(dio.residual().conditions.len(), 1, "the master refused it");

    let count = dio.value_scenery(Count::rows()).open().await?;
    assert_eq!(count.value(), int(2));
    Ok(())
}

/// Every cached row is in an unnarrowed Dio, so its count comes from the
/// cache's row count: no row is decoded on open or on a recompute.
#[tokio::test]
async fn an_unnarrowed_count_never_lists_the_cache() -> Result<()> {
    let (table, dio) = spied_dio().await?;
    let before = table.lists.load(Ordering::SeqCst);

    let count = dio.value_scenery(Count::rows()).open().await?;
    assert_eq!(count.value(), int(3));
    dio.apply_change(ChangeEvent::Inserted {
        id: "d".into(),
        new: Some(record("delta", 5)),
    })
    .await?;
    assert!(
        eventually(WITHIN, || count.value() == int(4)).await,
        "the count follows the insert: {:?}",
        count.value()
    );
    assert_eq!(table.lists.load(Ordering::SeqCst), before, "no cache list");
    Ok(())
}

/// A failed read flips the value to `Error` and keeps the number. The next
/// good read gives the same number, and still returns the status to `Fresh`.
#[tokio::test]
async fn a_value_recovers_from_a_failed_read_to_the_same_value() -> Result<()> {
    let (table, dio) = spied_dio().await?;
    let total = dio.value_scenery(Sum::of("price")).open().await?;
    assert_eq!(total.value(), int(60));

    table.fail_lists.store(true, Ordering::SeqCst);
    touch(&dio).await?;
    assert!(
        eventually(WITHIN, || matches!(total.status(), ValueStatus::Error(_))).await,
        "the failed read shows: {:?}",
        total.status()
    );
    assert_eq!(total.value(), int(60), "the last good value stays");

    table.fail_lists.store(false, Ordering::SeqCst);
    touch(&dio).await?;
    assert!(
        eventually(WITHIN, || total.status() == ValueStatus::Fresh).await,
        "an unchanged good read still recovers: {:?}",
        total.status()
    );
    assert_eq!(total.value(), int(60));
    Ok(())
}
