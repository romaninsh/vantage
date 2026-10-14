//! `dio.derive()` gathers every clause into one query and builds one Dio from
//! it, so a chain of clauses never builds a Dio per clause.

use vantage_core::Result;
use vantage_diorama::Dio;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Column, ColumnFlag, Condition, FilterOp, SortDirection, Vista, VistaMetadata, VistaQuery,
};

use crate::support::{MockView, eager_dio, record, teams_master};

/// Players with an `Orderable` name, on a mock that takes a condition, an
/// order and a search. Returns the master and a handle sharing its counters.
fn players() -> (Vista, MockShell) {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("team", "String"))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Orderable))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("p1", record(&[("id", "p1"), ("team", "red"), ("name", "Cara")]))
        .with_record("p2", record(&[("id", "p2"), ("team", "blue"), ("name", "Abe")]))
        .with_record("p3", record(&[("id", "p3"), ("team", "red"), ("name", "Dot")]))
        .with_record("p4", record(&[("id", "p4"), ("team", "red"), ("name", "Ann")]));
    (Vista::new("players", shell.clone()), shell)
}

fn red() -> Condition {
    Condition::new("team", FilterOp::Eq, "red")
}

/// Ids a view shows, in its order.
async fn ids(dio: &Dio, expected: usize) -> Vec<Option<String>> {
    let view = MockView::open(dio, 10).await;
    view.settle_until("narrowed rows", |v| v.loaded_rows() == expected)
        .await;
    (0..view.row_count()).map(|i| view.col_at(i, "id")).collect()
}

#[tokio::test]
async fn derive_chains_clauses_into_one_dio() -> Result<()> {
    let (master, probe) = players();
    let dio = eager_dio(master).await;
    let before = probe.calls().list;

    let johns = dio
        .derive()
        .with_condition(red())
        .with_order("name", SortDirection::Ascending)
        .with_search("a")
        .build()
        .await?;

    let query = johns.master().query();
    assert_eq!(query.conditions.len(), 1, "the master holds the condition");
    assert_eq!(
        query.order,
        Some(("name".to_string(), SortDirection::Ascending))
    );
    assert_eq!(query.search.as_deref(), Some("a"));
    assert!(johns.residual().is_empty(), "the mock took every clause");
    assert_eq!(
        probe.calls().list,
        before + 1,
        "one eager start: no Dio was built per clause"
    );
    #[cfg(feature = "diagnostics")]
    assert_eq!(
        johns.diagnostics().await?.cache_table,
        format!("players?{}", query.key()),
        "the cache table is keyed by the whole query"
    );
    assert_eq!(ids(&johns, 2).await, [Some("p4".into()), Some("p1".into())]);
    Ok(())
}

#[tokio::test]
async fn derive_with_query_takes_a_held_query() -> Result<()> {
    let chained = eager_dio(players().0)
        .await
        .derive()
        .with_condition(red())
        .with_order("name", SortDirection::Ascending)
        .with_search("a")
        .build()
        .await?;
    let held = VistaQuery::new()
        .with_condition(red())
        .with_order("name", SortDirection::Ascending)
        .with_search("a");
    let from_held = eager_dio(players().0)
        .await
        .derive()
        .with_query(&held)
        .build()
        .await?;

    assert_eq!(from_held.master().query().key(), chained.master().query().key());
    assert_eq!(ids(&from_held, 2).await, ids(&chained, 2).await);
    Ok(())
}

#[tokio::test]
async fn the_last_with_order_wins() -> Result<()> {
    let derived = eager_dio(players().0)
        .await
        .derive()
        .with_order("team", SortDirection::Ascending)
        .with_order("name", SortDirection::Descending)
        .build()
        .await?;
    assert_eq!(
        derived.master().query().order,
        Some(("name".to_string(), SortDirection::Descending))
    );
    Ok(())
}

#[tokio::test]
async fn with_query_extends_conditions_and_replaces_order_and_search() -> Result<()> {
    let held = VistaQuery::new()
        .with_condition(Condition::new("name", FilterOp::Ne, "Dot"))
        .with_order("name", SortDirection::Ascending)
        .with_search("a");
    let derived = eager_dio(players().0)
        .await
        .derive()
        .with_condition(red())
        .with_order("name", SortDirection::Descending)
        .with_search("x")
        .with_query(&held)
        .build()
        .await?;
    let expected = VistaQuery::new()
        .with_condition(red())
        .with_condition(Condition::new("name", FilterOp::Ne, "Dot"))
        .with_order("name", SortDirection::Ascending)
        .with_search("a");
    assert_eq!(derived.master().query(), &expected);
    Ok(())
}

#[tokio::test]
async fn an_empty_derive_carries_the_parent_query() -> Result<()> {
    let parent = eager_dio(players().0)
        .await
        .derive()
        .with_condition(red())
        .with_search("a")
        .build()
        .await?;
    let child = parent.derive().build().await?;
    assert_eq!(
        (child.master().query(), child.residual()),
        (parent.master().query(), parent.residual())
    );
    Ok(())
}

#[tokio::test]
async fn a_derive_error_surfaces_at_build() {
    let dio = eager_dio(teams_master()).await;
    let builder = dio
        .derive()
        .with_condition(Condition::new("colour", FilterOp::Eq, "red"));
    assert!(
        builder.build().await.is_err(),
        "`members` has no `colour` column"
    );
}

/// One query carries several clauses at once; no intermediate Dio is built.
#[tokio::test]
async fn derive_narrows_by_every_clause() {
    let dio = eager_dio(teams_master()).await;
    let query = VistaQuery::new()
        .with_condition(red())
        .with_order("id", SortDirection::Descending);
    let narrowed = dio
        .derive()
        .with_query(&query)
        .build()
        .await
        .expect("derives");

    assert_eq!(ids(&narrowed, 2).await, [Some("c".into()), Some("a".into())]);
}
