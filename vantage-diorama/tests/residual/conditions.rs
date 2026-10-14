//! A condition the master refuses, one test per kind of refusal: evaluated
//! over an eager view's complete cache, a loud failure over a paged view with
//! no eager start, and handed back by `Dio::fetch`.

use vantage_core::Result;
use vantage_diorama::{Clause, Dio, FilterOrigin};
use vantage_vista::{Capability, Condition, FilterOp, Vista, VistaQuery};

use super::fixtures::{NoBytes, assert_fails, items, open, wait_for_ids};
use crate::support::{eager_dio, paged_dio, paged_or_eager_dio};

/// Every capability a master needs to take a condition, bar `FilterOps`.
const NO_FILTER_OPS: &[Capability] = &[Capability::Order, Capability::Search];
const ALL: &[Capability] = &[Capability::Order, Capability::Search, Capability::FilterOps];

/// `condition` comes back in `Dio::fetch`'s `refused`.
async fn assert_refused(dio: &Dio, condition: Condition) -> Result<()> {
    let query = VistaQuery::new().with_condition(condition.clone());
    let fetched = dio.fetch(0..10, &query).await?;
    assert_eq!(fetched.refused.conditions, vec![condition]);
    Ok(())
}

/// The three shapes for one refused condition `column <op> value`: the eager
/// view shows `eager_ids`, the paged view fails naming `column`, and the
/// fetch hands the condition back.
async fn check(
    master: impl Fn() -> Vista,
    column: &str,
    op: FilterOp,
    value: ciborium::Value,
    eager_ids: &[&str],
) -> Result<()> {
    let eager = eager_dio(master()).await;
    let view = open(&eager, |b| {
        b.filter(Condition::new(column, op, value.clone()))
    })
    .await;
    wait_for_ids(&view, eager_ids).await;

    let paged = paged_dio(master()).await;
    let view = open(&paged, |b| {
        b.filter(Condition::new(column, op, value.clone()))
    })
    .await;
    assert_fails(&view, Clause::Filter(column.into()));

    assert_refused(&paged, Condition::new(column, op, value)).await
}

#[tokio::test]
async fn a_non_eq_operator_without_filter_ops() -> Result<()> {
    check(
        || items(NO_FILTER_OPS),
        "n",
        FilterOp::Gt,
        1.into(),
        &["b", "c"],
    )
    .await
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn a_computed_column() -> Result<()> {
    use super::fixtures::{metadata, shell};

    let master = || {
        let metadata = metadata(false).with_column(
            vantage_vista::Column::new("double", "Integer")
                .with_expression("row.n * 2")
                .expect("compiles"),
        );
        Vista::new("items", shell(metadata, ALL))
    };
    check(master, "double", FilterOp::Gt, 2.into(), &["b", "c"]).await
}

#[tokio::test]
async fn a_value_the_backend_cant_render() -> Result<()> {
    let master = || items(ALL).map_shell(|inner| NoBytes { inner });
    check(
        master,
        "blob",
        FilterOp::Eq,
        ciborium::Value::Bytes(vec![2]),
        &["b"],
    )
    .await
}

/// A dotted path the master refuses: the paged chunk filter fails loudly,
/// and a Lens with an eager start evaluates it over the whole copy.
#[tokio::test]
async fn a_dotted_path_the_master_refuses() -> Result<()> {
    check(|| items(ALL), "dims.w", FilterOp::Eq, 2.into(), &["b"]).await?;

    let fallback = paged_or_eager_dio(items(ALL)).await;
    let view = open(&fallback, |b| {
        b.filter(Condition::new("dims.w", FilterOp::Eq, 3))
    })
    .await;
    wait_for_ids(&view, &["c"]).await;
    Ok(())
}

/// A narrowed Dio's residual condition is part of what its views are: the
/// view stats list it as a query filter, ahead of the builder's, and the
/// runtime filters after both.
#[tokio::test]
async fn view_stats_list_the_dio_residual_first() -> Result<()> {
    let above_one = eager_dio(items(NO_FILTER_OPS))
        .await
        .with_query(&crate::support::cond("n", FilterOp::Gt, 1))
        .await?;
    assert_eq!(above_one.residual().conditions.len(), 1, "the master refused it");
    let view = open(&above_one, |b| {
        b.filter(Condition::new("n", FilterOp::Ne, 3))
    })
    .await;
    wait_for_ids(&view, &["b"]).await;
    view.set_filters(vec![Condition::new("n", FilterOp::Ne, 0)]);

    let listed: Vec<(FilterOp, ciborium::Value, FilterOrigin)> = view
        .view_stats()
        .filters
        .into_iter()
        .map(|f| (f.op, f.value, f.origin))
        .collect();
    assert_eq!(
        listed,
        vec![
            (FilterOp::Gt, 1.into(), FilterOrigin::Query),
            (FilterOp::Ne, 3.into(), FilterOrigin::Query),
            (FilterOp::Ne, 0.into(), FilterOrigin::View),
        ]
    );
    Ok(())
}

/// A condition on an augmented column, through a derived Dio: the master has
/// never heard of the column, so the condition stays with diorama, and the
/// two-pass view refines over its listed index as rows hydrate.
#[tokio::test]
async fn an_augmented_column_through_a_derived_dio() -> Result<()> {
    let parent = crate::support::bucket_dio().await;
    let johns = parent
        .with_query(&crate::support::cond("name", FilterOp::Eq, "John"))
        .await?;
    assert_eq!(johns.residual().conditions.len(), 1, "kept by diorama");

    let view = johns.table_scenery().page_size(10).open().await?;
    crate::support::wait_listed(&view).await;
    view.set_viewport(0..10, 0..0);
    let mut generations = view.subscribe();
    let hydrated = async {
        while super::fixtures::ids(&view) != ["o1", "o3"] {
            generations.changed().await.expect("generation watch");
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), hydrated)
        .await
        .expect("the two Johns surface as rows hydrate");

    let fetched = johns.fetch(0..10, &VistaQuery::new()).await?;
    assert_eq!(fetched.refused.conditions.len(), 1, "the residual is named");
    assert_eq!(fetched.refused.conditions[0].column, "name");
    Ok(())
}
