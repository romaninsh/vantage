use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Column, SortDirection, TableShell, Vista, VistaMetadata, VistaQuery,
};

use super::ViewPlan;
use crate::{Cache, Dio, Lens};

/// Two rows under `id` and `team`; `team` isn't orderable.
fn teams(searchable: bool) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("team", "String"))
        .with_id_column("id");
    let row = |id: &str, team: &str| -> vantage_types::Record<ciborium::Value> {
        [("id", id), ("team", team)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), ciborium::Value::Text(v.to_string())))
            .collect()
    };
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", row("a", "red"))
        .with_record("b", row("b", "blue"));
    let shell = if searchable {
        shell
    } else {
        let caps = shell.capabilities().without(Capability::Search);
        shell.with_capabilities(caps)
    };
    Vista::new("teams", shell)
}

async fn dio(master: Vista) -> Dio {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .build()
        .expect("lens builds");
    lens.dio(master).build().await.expect("dio builds")
}

fn plan(dio: &Dio, query: VistaQuery) -> ViewPlan {
    ViewPlan::new(&dio.inner, query).expect("plans")
}

#[tokio::test]
async fn a_query_the_master_takes_whole_has_no_residual() {
    let dio = dio(teams(true)).await;
    let plan = plan(&dio, VistaQuery::new().with_search("re"));
    assert!(!plan.has_residual());
    assert!(plan.residual().is_empty());
}

#[tokio::test]
async fn a_refused_order_is_a_residual_and_not_pushed() {
    let dio = dio(teams(true)).await;
    let plan = plan(
        &dio,
        VistaQuery::new().with_order("team", SortDirection::Ascending),
    );
    assert!(plan.has_residual());
    assert!(plan.residual().order.is_some());
}

#[tokio::test]
async fn the_dio_residual_search_alone_is_a_residual() {
    let derived = dio(teams(false))
        .await
        .with_query(&VistaQuery::new().with_search("x"))
        .await
        .expect("derives");
    let plan = plan(&derived, VistaQuery::new());
    assert!(plan.has_residual());
    assert_eq!(plan.residual().search.as_deref(), Some("x"));
}

#[tokio::test]
async fn with_both_searches_left_the_residual_names_the_view_search() {
    let derived = dio(teams(false))
        .await
        .with_query(&VistaQuery::new().with_search("x"))
        .await
        .expect("derives");
    let plan = plan(&derived, VistaQuery::new().with_search("y"));
    assert!(plan.has_residual());
    assert_eq!(plan.residual().search.as_deref(), Some("y"));
}
