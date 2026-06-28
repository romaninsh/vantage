//! Stage 4: a traversed (`get_ref`) child Dio is first-class — it carries the
//! Dio-level query semantics from Stages 1/3, so a condition set on the child
//! filters its rows locally, exactly like a top-level Dio. This is the
//! composition of reference traversal with local emulation.

mod support;

use ciborium::Value as CborValue;
use support::MockView;
use vantage_diorama::{Dio, Lens};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Reference, ReferenceKind, Vista, VistaMetadata};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), text(v))).collect()
}

/// Crew store keyed by `launch_id`: L1 has two members, L2 one.
fn crew_shell() -> MockShell {
    let meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("launch_id", "String"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    MockShell::new()
        .with_metadata(meta)
        .with_record("c1", rec(&[("id", "c1"), ("launch_id", "L1"), ("name", "Buzz")]))
        .with_record("c2", rec(&[("id", "c2"), ("launch_id", "L1"), ("name", "Neil")]))
        .with_record("c3", rec(&[("id", "c3"), ("launch_id", "L2"), ("name", "Pete")]))
}

fn launch_master() -> Vista {
    let meta = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id")
        .with_reference(Reference::new("crew", "launch_crew", ReferenceKind::HasMany, "launch_id"));
    let shell = MockShell::new()
        .with_metadata(meta)
        .with_record("L1", rec(&[("id", "L1"), ("name", "Apollo 11")]))
        .with_record("L2", rec(&[("id", "L2"), ("name", "Apollo 12")]))
        .with_ref_target("crew", crew_shell());
    Vista::new("launches", Box::new(shell))
}

/// Single-pass eager Dio over the launches master (in-memory cache).
async fn launches_dio() -> Dio {
    let lens = std::sync::Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    use vantage_dataset::prelude::ReadableValueSet;
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("lens builds"),
    );
    lens.make_dio(launch_master()).await.expect("make_dio")
}

#[tokio::test]
async fn traversed_child_carries_a_local_condition() {
    let launches = launches_dio().await;
    let l1 = rec(&[("id", "L1"), ("name", "Apollo 11")]);

    // Traverse launch L1 → its crew (two members), then narrow with a Dio-level
    // condition on the child. The child reuses the parent's lens and filters
    // locally just like a top-level Dio.
    let crew = launches.get_ref("crew", &l1).await.expect("get_ref crew");
    crew.with_condition_eq("name", "Buzz");

    let view = MockView::open(&crew, 10).await;
    view.settle_until("child filtered to Buzz", |v| v.loaded_rows() == 1)
        .await;

    assert_eq!(view.loaded_rows(), 1, "only Buzz matches on L1's crew");
    assert_eq!(view.col_at(0, "name").as_deref(), Some("Buzz"));
}

#[tokio::test]
async fn traversed_child_without_condition_sees_all_related_rows() {
    let launches = launches_dio().await;
    let l1 = rec(&[("id", "L1"), ("name", "Apollo 11")]);
    let crew = launches.get_ref("crew", &l1).await.expect("get_ref crew");

    let view = MockView::open(&crew, 10).await;
    view.settle_until("both L1 crew", |v| v.loaded_rows() == 2).await;
    assert_eq!(view.loaded_rows(), 2, "L1 has two crew members");
}
