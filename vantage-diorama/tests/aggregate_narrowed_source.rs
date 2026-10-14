//! Aggregates over a derived Dio count only the rows the derivation admits,
//! whether the master took the narrowing or the Dio holds it as a residual.

mod support;

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::aggregate::{Conditions, Count, GroupBy, GroupReducer, Reduce, Where};
use vantage_diorama::{Dio, ValueScenery};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Capability, Column, Condition, FilterOp, SortDirection, VistaCapabilities, VistaMetadata,
};

use support::aggregate::{as_i64, int, master, record, settle, source, text};

/// Two open orders, one done.
fn orders(shell: MockShell) -> MockShell {
    shell.set_record("a", record(&[("status", text("open"))]));
    shell.set_record("b", record(&[("status", text("done"))]));
    shell.set_record("c", record(&[("status", text("open"))]));
    shell
}

fn count_by_status() -> GroupBy<impl GroupReducer> {
    count_by("status")
}

/// A row count and a per-status count mounted over `open`, both settled.
/// The derived Dio is held so it keeps following.
struct Mounted {
    _groups: Dio,
    count: Arc<dyn ValueScenery>,
    groups: IndexMap<String, Record<CborValue>>,
}

/// Mount both aggregates over `open` and check each admits only the two open
/// orders.
async fn mount_two_open(open: &Dio) -> Result<Mounted> {
    let count = open.value_scenery(Count::rows()).open().await?;
    let derived = open.aggregate(count_by_status()).await?;
    settle().await;
    assert_eq!(as_i64(count.value()), Some(2));
    let groups = derived.cached().list_values().await?;
    assert_eq!(groups.keys().collect::<Vec<_>>(), ["open"]);
    Ok(Mounted {
        _groups: derived,
        count,
        groups,
    })
}

/// The master takes the condition, so the narrowed Dio's cache holds only
/// the open orders.
#[tokio::test(flavor = "multi_thread")]
async fn narrowed_dio_aggregates_the_narrowed_set() -> Result<()> {
    let shell = orders(master(&[("status", "String")]));
    let src = source(shell.clone()).await;
    let open = src
        .derive()
        .with_condition(Condition::new("status", FilterOp::Eq, "open"))
        .build()
        .await?;
    let mounted = mount_two_open(&open).await?;

    shell.set_record("d", record(&[("status", text("open"))]));
    shell.set_record("e", record(&[("status", text("done"))]));
    open.refresh().await?;
    settle().await;
    assert_eq!(
        as_i64(mounted.count.value()),
        Some(3),
        "follows the narrowed Dio"
    );
    Ok(())
}

/// A master without `Search` refuses the search, so it becomes the derived
/// Dio's residual: its cache holds every order, and only the facade applies
/// the search. An aggregate reading the cache directly would count three.
#[tokio::test(flavor = "multi_thread")]
async fn a_value_and_a_derive_see_a_residual_narrowing() -> Result<()> {
    let caps: VistaCapabilities = [Capability::Order, Capability::Window]
        .into_iter()
        .collect();
    let shell = orders(master(&[("status", "String")]).with_capabilities(caps));
    let src = source(shell).await;
    let open = src
        .derive()
        .with_search("open")
        .build()
        .await?;
    assert!(
        open.residual().search.is_some(),
        "the master refused the search"
    );
    let mounted = mount_two_open(&open).await?;
    assert_eq!(mounted.groups["open"].get("n"), Some(&int(2)));
    Ok(())
}

/// A group reducer sees its rows in the source Dio's order, not the order
/// the cache happens to hold them in.
#[tokio::test(flavor = "multi_thread")]
async fn a_group_sees_its_rows_in_the_dio_order() -> Result<()> {
    let shell = master(&[("status", "String"), ("amount", "i64")]);
    for (id, amount) in [("a", 1), ("b", 3), ("c", 2)] {
        shell.set_record(
            id,
            record(&[("status", text("open")), ("amount", int(amount))]),
        );
    }
    let src = source(shell).await;
    let by_amount = src
        .derive()
        .with_order("amount", SortDirection::Descending)
        .build()
        .await?;
    let first = Reduce::new(vec![Column::new("first", "i64")], |_key, rows, _schema| {
        let mut out = Record::new();
        let amount = rows[0].get("amount").cloned().unwrap_or(CborValue::Null);
        out.insert("first".to_string(), amount);
        out
    });
    let derived = by_amount
        .aggregate(GroupBy::column("status", first))
        .await?;
    let groups = derived.cached().list_values().await?;
    assert_eq!(groups["open"].get("first"), Some(&int(3)), "{groups:?}");
    Ok(())
}

/// The derived key column keeps the source's declared type, so a `Where`
/// over the grouped Dio coerces against `i64`, and agrees with the grouped
/// Dio's own evaluator.
#[tokio::test(flavor = "multi_thread")]
async fn a_where_over_an_integer_group_key_counts_the_group() -> Result<()> {
    let shell = master(&[("code", "i64")]);
    for (id, code) in [("a", 200), ("b", 200), ("c", 500)] {
        shell.set_record(id, record(&[("code", int(code))]));
    }
    let src = source(shell).await;
    let by_code = src.aggregate(count_by("code")).await?;
    settle().await;

    let key = by_code.master().column("code").cloned();
    let key = key.expect("the derived Dio declares its key");
    assert_eq!(key.original_type.as_deref(), Some("i64"));

    let at_200 = Where::new(Conditions::new().eq("code", 200), Count::rows());
    let count = by_code.value_scenery(at_200).open().await?;
    settle().await;
    assert_eq!(as_i64(count.value()), Some(1));

    let residual = Condition::new("code", FilterOp::Eq, 200);
    let schema = VistaMetadata::new().with_column(key);
    let groups = by_code.cached().list_values().await?;
    let matched: Vec<&String> = groups
        .iter()
        .filter(|(_, row)| residual.matches(row, &schema))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(matched, ["200"]);
    Ok(())
}

/// One row per distinct `key`, counting its members as `n`.
fn count_by(key: &str) -> GroupBy<impl GroupReducer> {
    let reducer = Reduce::new(vec![Column::new("n", "i64")], |_key, rows, _schema| {
        let mut out = Record::new();
        out.insert("n".to_string(), int(rows.len() as i64));
        out
    });
    GroupBy::column(key, reducer)
}
