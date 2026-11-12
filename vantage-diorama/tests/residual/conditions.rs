//! A condition the master refuses, one test per kind of refusal: evaluated
//! over an eager view's complete cache, a loud failure over a paged view with
//! no eager start, and handed back by `Dio::fetch`.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Clause, Dio, FilterOrigin};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Column, Condition, FilterOp, TableShell, Vista, VistaMetadata, VistaQuery,
    forward_table_shell,
};

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
        .derive()
        .with_condition(Condition::new("n", FilterOp::Gt, 1))
        .build()
        .await?;
    assert_eq!(
        above_one.residual().conditions.len(),
        1,
        "the master refused it"
    );
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

/// The epoch pair SurrealDB stores a datetime as (`Tag(12)`), for the dates
/// [`Dates`] knows.
fn epoch(date: &str) -> Option<CborValue> {
    let secs: i64 = match date {
        "2026-11-20" => 1_795_132_800,
        "2026-12-01" => 1_796_083_200,
        "2026-12-10" => 1_796_860_800,
        _ => return None,
    };
    Some(CborValue::Tag(
        12,
        Box::new(CborValue::Array(vec![secs.into(), 0.into()])),
    ))
}

/// A master that stores `due` as a typed datetime and can't filter on it: a
/// condition on `due` is refused, and compared as the datetime its text
/// names.
#[derive(Clone)]
struct Dates {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Dates, inner, {
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.column == "due" {
            return vantage_vista::unsupported(Capability::FilterOps);
        }
        self.inner.add_condition(condition)
    }
    fn comparable_condition(&self, condition: &Condition) -> Condition {
        let mut typed = condition.clone();
        if let (CborValue::Text(date), "due") = (&condition.value, condition.column.as_str())
            && let Some(value) = epoch(date)
        {
            typed.value = value;
        }
        typed
    }
});

/// `a`, `b` and `c` due on 2026-11-20, 2026-12-01 and 2026-12-10, held typed.
fn dated() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("due", "DateTime"))
        .with_id_column("id");
    let shell = [
        ("a", "2026-11-20"),
        ("b", "2026-12-01"),
        ("c", "2026-12-10"),
    ]
    .into_iter()
    .fold(
        MockShell::new().with_metadata(metadata),
        |shell, (id, due)| {
            let mut row = Record::new();
            row.insert("id".into(), CborValue::Text(id.into()));
            row.insert("due".into(), epoch(due).expect("a known date"));
            shell.with_record(id, row)
        },
    );
    Vista::new("dated", shell).map_shell(|inner| Dates { inner })
}

async fn listed(vista: &Vista) -> Result<Vec<String>> {
    let mut ids: Vec<String> = vista.list_values().await?.into_keys().collect();
    ids.sort();
    Ok(ids)
}

/// A date condition the master refuses is judged in memory against the
/// datetime the master would compare, not the text the filter gave: through
/// a derived Dio's residual, a view's filter over the cache, and a facade
/// over another Dio.
#[tokio::test]
async fn a_refused_date_condition_compares_the_masters_typed_operand() -> Result<()> {
    let parent = eager_dio(dated()).await;
    for (op, want) in [(FilterOp::Eq, &["b"][..]), (FilterOp::Gte, &["b", "c"])] {
        let condition = Condition::new("due", op, "2026-12-01");
        let derived = parent
            .derive()
            .with_condition(condition.clone())
            .build()
            .await?;
        assert_eq!(derived.residual().conditions, vec![condition.clone()]);
        assert_eq!(listed(&derived.cached()).await?, want, "{op:?} residual");
        wait_for_ids(&open(&derived, |b| b).await, want).await;

        let view = open(&parent, |b| b.filter(condition.clone())).await;
        wait_for_ids(&view, want).await;

        let facade = parent.cached().with_condition("due", op, "2026-12-01")?;
        assert_eq!(
            facade.comparable_query().conditions[0].value,
            epoch("2026-12-01").unwrap(),
            "the facade compares as its master does"
        );
        assert_eq!(listed(&facade).await?, want, "{op:?} facade");
    }
    Ok(())
}

/// A condition on an augmented column, through a derived Dio: the master has
/// never heard of the column, so the condition stays with diorama, and the
/// two-pass view refines over its listed index as rows hydrate.
#[tokio::test]
async fn an_augmented_column_through_a_derived_dio() -> Result<()> {
    let parent = crate::support::bucket_dio().await;
    let johns = parent
        .derive()
        .with_condition(Condition::new("name", FilterOp::Eq, "John"))
        .build()
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
