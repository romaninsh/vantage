//! What the derived Vista's order, search and conditions mean over ragged
//! values.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{Column, ColumnFlag, FilterOp, SortDirection, Vista};

use super::AggregateShell;
use crate::aggregate::aggregation::DerivedRows;
use crate::aggregate::derived::aggregate_rows;

fn row(id: &str, v: Option<CborValue>) -> (String, Record<CborValue>) {
    let mut record = Record::new();
    record.insert("id".to_string(), CborValue::Text(id.to_string()));
    if let Some(v) = v {
        record.insert("v".to_string(), v);
    }
    (id.to_string(), record)
}

fn vista(rows: Vec<(String, Record<CborValue>)>) -> Vista {
    let columns = vec![
        Column::new("id", "String").with_flag(ColumnFlag::Id),
        Column::new("v", "any"),
    ];
    let shell = AggregateShell::new(
        aggregate_rows(DerivedRows::new(rows, columns.clone())),
        &columns,
    );
    Vista::new("derived", shell)
}

async fn ids(vista: Vista) -> Result<Vec<String>> {
    Ok(vista.list_values().await?.into_keys().collect())
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

#[tokio::test]
async fn order_is_numeric_across_int_and_float() -> Result<()> {
    let v = vista(vec![
        row("a", Some(CborValue::Float(657.96))),
        row("b", Some(CborValue::Float(1826.19))),
        row("c", Some(int(2))),
        row("d", Some(CborValue::Float(10.5))),
    ]);
    let ordered = v.with_order("v", SortDirection::Ascending)?;
    assert_eq!(ids(ordered).await?, ["c", "d", "a", "b"]);
    Ok(())
}

#[tokio::test]
async fn absent_and_null_cells_sort_last_in_both_directions() -> Result<()> {
    let rows = vec![
        row("absent", None),
        row("null", Some(CborValue::Null)),
        row("one", Some(int(1))),
        row("two", Some(int(2))),
    ];
    let asc = vista(rows.clone()).with_order("v", SortDirection::Ascending)?;
    assert_eq!(ids(asc).await?, ["one", "two", "absent", "null"]);
    let desc = vista(rows).with_order("v", SortDirection::Descending)?;
    assert_eq!(ids(desc).await?, ["two", "one", "absent", "null"]);
    Ok(())
}

/// Without the id tiebreak, rows sharing a value would keep their input
/// order, and the same data assembled differently would render differently.
#[tokio::test]
async fn ties_break_on_id_in_both_directions() -> Result<()> {
    let rows = vec![row("b", Some(int(1))), row("a", Some(int(1)))];
    for dir in [SortDirection::Ascending, SortDirection::Descending] {
        let ordered = vista(rows.clone()).with_order("v", dir)?;
        assert_eq!(ids(ordered).await?, ["a", "b"], "{dir:?}");
    }
    Ok(())
}

/// A positive NaN orders above every number, so the sort is total and
/// doesn't panic.
#[tokio::test]
async fn a_nan_cell_sorts_above_every_number() -> Result<()> {
    let v = vista(vec![
        row("nan", Some(CborValue::Float(f64::NAN))),
        row("one", Some(int(1))),
    ]);
    let ordered = v.with_order("v", SortDirection::Ascending)?;
    assert_eq!(ids(ordered).await?, ["one", "nan"]);
    Ok(())
}

#[tokio::test]
async fn search_is_case_insensitive_and_reads_nested_and_numeric_cells() -> Result<()> {
    let nested = CborValue::Map(vec![(
        CborValue::Text("note".into()),
        CborValue::Text("Fragile".into()),
    )]);
    let rows = vec![
        row("nested", Some(nested)),
        row("number", Some(int(4217))),
        row("other", Some(CborValue::Text("plain".into()))),
    ];
    assert_eq!(
        ids(vista(rows.clone()).with_search("FRAG")?).await?,
        ["nested"]
    );
    assert_eq!(ids(vista(rows).with_search("421")?).await?, ["number"]);
    Ok(())
}

#[tokio::test]
async fn an_eq_condition_takes_an_int_and_a_float_as_equal() -> Result<()> {
    let rows = vec![
        row("int", Some(int(3))),
        row("float", Some(CborValue::Float(3.0))),
        row("absent", None),
    ];
    let narrowed = vista(rows).with_condition("v", FilterOp::Eq, CborValue::Float(3.0))?;
    assert_eq!(ids(narrowed).await?, ["int", "float"]);
    Ok(())
}
