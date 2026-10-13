//! Aggregates over a derived Dio count only the rows the derivation admits,
//! whether the master took the narrowing or the Dio holds it as a residual.

mod support;

use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Dio, ValueScenery};
use vantage_diorama_aggregate::{AggregateLens, Count, DerivedDio, GroupBy, Reduce};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Capability, Column, Condition, FilterOp, VistaCapabilities, VistaQuery};

use support::{as_i64, int, lens, master, record, settle, source, text};

/// Two open orders, one done.
fn orders(shell: MockShell) -> MockShell {
    shell.set_record("a", record(&[("status", text("open"))]));
    shell.set_record("b", record(&[("status", text("done"))]));
    shell.set_record("c", record(&[("status", text("open"))]));
    shell
}

fn count_by_status() -> GroupBy<impl vantage_diorama_aggregate::GroupReducer> {
    let reducer = Reduce::new(vec![Column::new("n", "i64")], |_key, rows| {
        let mut out = Record::new();
        out.insert("n".to_string(), int(rows.len() as i64));
        out
    });
    GroupBy::column("status", reducer)
}

/// A row count and a per-status count mounted over `open`, both settled.
/// The lens and the derived Dio are held so the count keeps following.
struct Mounted {
    _agg: Arc<AggregateLens>,
    _groups: DerivedDio,
    count: Arc<dyn ValueScenery>,
    groups: IndexMap<String, Record<CborValue>>,
}

/// Mount both aggregates over `open` and check each admits only the two open
/// orders.
async fn mount_two_open(open: &Dio) -> Result<Mounted> {
    let agg = lens();
    let count = agg.value(open, "open#count", Count::rows()).await?;
    let derived = agg
        .derive(open, "open#by_status", count_by_status())
        .await?;
    settle().await;
    assert_eq!(as_i64(count.value()), Some(2));
    let groups = derived.cached().list_values().await?;
    assert_eq!(groups.keys().collect::<Vec<_>>(), ["open"]);
    Ok(Mounted {
        _agg: agg,
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
        .with_query(&VistaQuery::new().with_condition(Condition::new(
            "status",
            FilterOp::Eq,
            "open",
        )))
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
        .with_query(&VistaQuery::new().with_search("open"))
        .await?;
    assert!(
        open.residual().search.is_some(),
        "the master refused the search"
    );
    let mounted = mount_two_open(&open).await?;
    assert_eq!(mounted.groups["open"].get("n"), Some(&int(2)));
    Ok(())
}
