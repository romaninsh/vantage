//! Opening a view: rows from the cache, life past the Dio handle, and
//! whether the plan can search.

use tempfile::TempDir;
use vantage_core::Result;
use vantage_vista::{Capability, Column, ColumnFlag, Vista, VistaMetadata, mocks::MockShell};

use super::{build_lens, seeded, seeded_master};
use crate::support::{eager_dio, paged_dio, paged_or_eager_dio};

#[tokio::test]
async fn scenery_loads_rows_from_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    assert_eq!(scenery.row_count(), 3);
    assert!(scenery.row(0).is_some());
    assert!(scenery.row(2).is_some());
    assert!(scenery.row(3).is_none());
    Ok(())
}

#[tokio::test]
async fn scenery_outlives_dio_handle_drop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = build_lens(tmp.path().join("cache.redb")).await?;
    let dio = lens.dio(seeded_master()).build().await?;

    let scenery = dio.table_scenery().open().await?;
    assert_eq!(scenery.row_count(), 3);

    // Drop the external Dio handle. Scenery still has its rows in
    // memory (just no future reloads).
    drop(dio);
    assert_eq!(scenery.row_count(), 3);
    Ok(())
}

/// A searchable master without `Search`.
fn unsearchable_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Searchable))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_capabilities([Capability::Window].into_iter().collect())
        .with_record("a", seeded("a", "alpha", 30, "open", 1));
    Vista::new("items", shell)
}

#[tokio::test]
async fn search_supported_follows_the_plan() -> Result<()> {
    let paged = paged_dio(unsearchable_master()).await;
    let view = paged.table_scenery().open().await?;
    assert!(
        !view.search_supported(),
        "a window can't be searched locally"
    );

    let copied = paged_or_eager_dio(unsearchable_master()).await;
    let view = copied.table_scenery().open().await?;
    assert!(view.search_supported(), "the whole copy can be searched");

    let eager = eager_dio(unsearchable_master()).await;
    let view = eager.table_scenery().open().await?;
    assert!(view.search_supported());
    Ok(())
}
