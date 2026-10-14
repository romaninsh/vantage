//! Behaviour of the aggregation layer against a live source: the pure
//! reductions here, the value sceneries in `values`, the derived Dios in
//! `derived`.

#[path = "../support/mod.rs"]
mod support;

mod derived;
mod values;

use vantage_diorama::aggregate::{Aggregation, DerivedRows, GroupBy, Rows, Sum};
use vantage_vista::VistaMetadata;

use support::aggregate::{int, record, revenue, text};

/// The property the whole design rests on: `compute` is a pure function, so the
/// same rows always give the same answer regardless of how they were assembled.
#[tokio::test]
async fn compute_is_deterministic_across_input_orderings() {
    let forwards: Rows = [
        (
            "a".to_string(),
            record(&[("status", text("open")), ("amount", int(3))]),
        ),
        (
            "b".to_string(),
            record(&[("status", text("done")), ("amount", int(5))]),
        ),
        (
            "c".to_string(),
            record(&[("status", text("open")), ("amount", int(7))]),
        ),
    ]
    .into_iter()
    .collect();
    let backwards: Rows = forwards
        .iter()
        .rev()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let schema = VistaMetadata::new();
    let grouped = GroupBy::column("status", revenue());
    assert_eq!(
        grouped.compute(&forwards, &schema),
        grouped.compute(&backwards, &schema),
        "grouping must not depend on input order",
    );
    assert_eq!(Sum::of("amount").compute(&forwards, &schema), int(15));
}

/// A row set with a stable schema but no rows still carries its columns, so a
/// grid over an empty group-by renders headers rather than nothing.
#[tokio::test]
async fn derived_rows_carry_their_schema_when_empty() {
    let empty: Rows = Rows::new();
    let out: DerivedRows =
        GroupBy::column("status", revenue()).compute(&empty, &VistaMetadata::new());
    assert!(out.is_empty());
    assert_eq!(
        out.columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        vec!["status", "orders", "revenue"],
    );
}
