//! Reference traversal: `Dio::get_ref` returns a new Dio bound to the traversed
//! target Vista, and stays resilient when that target's source is unreachable —
//! a temporarily-dead relation source must never read as "no such reference".

use std::sync::Arc;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::Lens;
use vantage_types::Record;
use vantage_vista::{mocks::MockShell, Column, Reference, ReferenceKind, Vista, VistaMetadata};

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

/// A crew store keyed by `launch_id`, with one member on each of two launches.
fn crew_shell() -> MockShell {
    let meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("launch_id", "String"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    MockShell::new()
        .with_metadata(meta)
        .with_record("c1", rec(&[("id", "c1"), ("launch_id", "L1"), ("name", "Buzz")]))
        .with_record("c2", rec(&[("id", "c2"), ("launch_id", "L2"), ("name", "Neil")]))
}

/// A launches master declaring a `crew` has-many onto `launch_crew`, resolved
/// against the given crew store.
fn launch_master(crew: MockShell) -> Vista {
    let meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id")
        .with_reference(Reference::new(
            "crew",
            "launch_crew",
            ReferenceKind::HasMany,
            "launch_id",
        ));
    let shell = MockShell::new()
        .with_metadata(meta)
        .with_record("L1", rec(&[("id", "L1"), ("name", "Apollo 11")]))
        .with_record("L2", rec(&[("id", "L2"), ("name", "Apollo 12")]))
        .with_ref_target("crew", crew);
    Vista::new("launches", Box::new(shell))
}

/// Warm-cache-aware eager lens (mirrors the UI's `build_eager_lens`): copy the
/// master into the cache on first open, but TRUST a warm cache — so a target
/// whose source is down still opens from previously-cached rows.
fn eager_lens(cache: std::path::PathBuf) -> Arc<Lens> {
    Arc::new(
        Lens::new()
            .cache_at(cache)
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    if dio.cache().count().await? > 0 {
                        return Ok(());
                    }
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    )
}

#[tokio::test]
async fn get_ref_resolves_and_lists_the_related_rows() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("cache.redb"));
    let launches = lens.make_dio(launch_master(crew_shell())).await?;

    let l1 = rec(&[("id", "L1"), ("name", "Apollo 11")]);
    let crew = launches.get_ref("crew", &l1).await?;

    let scenery = crew.table_scenery().open().await?;
    assert_eq!(scenery.row_count(), 1, "L1 has exactly one crew member");
    Ok(())
}

#[tokio::test]
async fn get_ref_undefined_relation_errors_synchronously() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("cache.redb"));
    let launches = lens.make_dio(launch_master(crew_shell())).await?;
    let l1 = rec(&[("id", "L1")]);
    assert!(
        launches.get_ref("nope", &l1).await.is_err(),
        "an undefined relation is the only legitimate get_ref failure"
    );
    Ok(())
}

#[tokio::test]
async fn warm_retraversal_survives_a_dead_target_source() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("cache.redb"));
    let crew = crew_shell();
    let crew_handle = crew.clone(); // shares the fail toggle + data via Arc
    let launches = lens.make_dio(launch_master(crew)).await?;
    let l1 = rec(&[("id", "L1")]);

    // First traversal warms the per-parent crew cache.
    let crew_dio = launches.get_ref("crew", &l1).await?;
    assert_eq!(crew_dio.table_scenery().open().await?.row_count(), 1);

    // The crew source goes down (a 503 on refresh).
    crew_handle.set_fail_reads(true);

    // Re-traverse: still returns a Dio (never "no ref 'crew'") and still shows
    // the cached crew member — the warm cache is trusted, the dead source is
    // never touched.
    let crew_again = launches.get_ref("crew", &l1).await?;
    assert_eq!(crew_again.table_scenery().open().await?.row_count(), 1);
    Ok(())
}

#[tokio::test]
async fn each_parent_gets_an_isolated_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = eager_lens(tmp.path().join("cache.redb"));
    let launches = lens.make_dio(launch_master(crew_shell())).await?;

    let l1 = rec(&[("id", "L1")]);
    let l2 = rec(&[("id", "L2")]);
    let crew1 = launches.get_ref("crew", &l1).await?;
    let crew2 = launches.get_ref("crew", &l2).await?;

    assert_ne!(
        crew1.cache_table_name(),
        crew2.cache_table_name(),
        "each parent's crew lives in its own cache table"
    );
    assert_eq!(crew1.table_scenery().open().await?.row_count(), 1);
    assert_eq!(crew2.table_scenery().open().await?.row_count(), 1);
    Ok(())
}
