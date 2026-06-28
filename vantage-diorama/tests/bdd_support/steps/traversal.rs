//! Reference-traversal steps: `dio.get_ref(relation, row)` returns a new Dio
//! bound to the traversed target, and stays resilient when that target's source
//! is temporarily unreachable — a dead relation source must never read as "no
//! such reference".

use std::sync::Arc;

use ciborium::Value as CborValue;
use cucumber::{given, then, when};
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::Lens;
use vantage_types::Record;
use vantage_vista::{mocks::MockShell, Column, Reference, ReferenceKind, Vista, VistaMetadata};

use crate::bdd_support::world::DioramaWorld;

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

/// A `launch_crew` store keyed by `launch_id`, one member on each of two launches.
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

/// A `launches` master declaring a `crew` has-many onto `launch_crew`.
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

#[given("a launch master with a crew reference behind a warm-cache lens")]
async fn launch_with_crew(w: &mut DioramaWorld) {
    // Warm-cache-aware eager lens (mirrors the UI's `build_eager_lens`): copy
    // the master into the cache on first open, but TRUST a warm cache — so a
    // target whose source has gone down still opens from previously-cached rows.
    let cache = w.cache_source();
    let lens = Arc::new(
        Lens::new()
            .cache_source(cache)
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
    );

    let crew = crew_shell();
    // Keep a handle that shares the fail toggle + store, so a later step can
    // take the crew source offline.
    w.ref_source = Some(crew.clone());

    let dio = lens
        .make_dio(launch_master(crew))
        .await
        .expect("make launch dio");
    w.lens = Some(lens);
    w.dio = Some(dio);
}

#[when(regex = r#"^the "([^"]+)" reference is traversed from launch "([^"]+)"$"#)]
async fn traverse(w: &mut DioramaWorld, relation: String, launch_id: String) {
    let dio = w.dio.as_ref().expect("launch dio not created");
    let row = rec(&[("id", launch_id.as_str())]);
    match dio.get_ref(&relation, &row).await {
        Ok(d) => {
            w.traversed = Some(d);
            w.last_error = None;
        }
        Err(e) => {
            w.traversed = None;
            w.last_error = Some(e.to_string());
        }
    }
}

#[when("the crew source goes offline")]
async fn crew_offline(w: &mut DioramaWorld) {
    w.ref_source
        .as_ref()
        .expect("no crew source handle")
        .set_fail_reads(true);
}

#[then(regex = r"^the traversed dio lists (\d+) crew members?$")]
async fn traversed_lists(w: &mut DioramaWorld, expected: usize) {
    let dio = w.traversed.as_ref().expect("no traversed dio");
    let scenery = dio.table_scenery().open().await.expect("open scenery");
    assert_eq!(
        scenery.row_count(),
        expected,
        "expected {expected} crew members, got {}",
        scenery.row_count()
    );
}

#[then(regex = r#"^traversing "([^"]+)" from launch "([^"]+)" fails$"#)]
async fn traverse_fails(w: &mut DioramaWorld, relation: String, launch_id: String) {
    let dio = w.dio.as_ref().expect("launch dio not created");
    let row = rec(&[("id", launch_id.as_str())]);
    assert!(
        dio.get_ref(&relation, &row).await.is_err(),
        "expected traversing '{relation}' to fail"
    );
}

#[then(regex = r#"^crew caches for launches "([^"]+)" and "([^"]+)" are distinct$"#)]
async fn caches_distinct(w: &mut DioramaWorld, a: String, b: String) {
    let dio = w.dio.as_ref().expect("launch dio not created");
    let crew_a = dio
        .get_ref("crew", &rec(&[("id", a.as_str())]))
        .await
        .expect("traverse a");
    let crew_b = dio
        .get_ref("crew", &rec(&[("id", b.as_str())]))
        .await
        .expect("traverse b");
    assert_ne!(
        crew_a.cache_table_name(),
        crew_b.cache_table_name(),
        "each launch's crew must live in its own cache table"
    );
}
