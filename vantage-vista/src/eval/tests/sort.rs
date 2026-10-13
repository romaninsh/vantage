//! `VistaQuery::sort_rows`: absent and null last, ties on id, cells read
//! as conditions read them.

use super::*;
use crate::SortDirection;

type Rows = Vec<(String, Record<CborValue>)>;

fn ids(rows: &Rows) -> Vec<&str> {
    rows.iter().map(|(id, _)| id.as_str()).collect()
}

fn sorted(mut rows: Rows, column: &str, dir: SortDirection, metadata: &VistaMetadata) -> Rows {
    VistaQuery::new()
        .with_order(column, dir)
        .sort_rows(&mut rows, metadata);
    rows
}

/// Rows `a`..`f`: `c` lacks the price, `f` holds a null, `b` and `d` tie.
fn priced() -> Rows {
    let price = |id: &str, p: Option<CborValue>| {
        let cells: Vec<(&str, CborValue)> = p.into_iter().map(|p| ("price", p)).collect();
        (id.to_string(), row(&cells))
    };
    vec![
        price("e", Some(int(1))),
        price("f", Some(CborValue::Null)),
        price("d", Some(CborValue::Float(2.5))),
        price("c", None),
        price("b", Some(CborValue::Float(2.5))),
        price("a", Some(int(10))),
    ]
}

#[test]
fn absent_and_null_cells_sort_last_both_ways_and_ties_break_on_id() {
    let m = schemaless();
    let asc = sorted(priced(), "price", SortDirection::Ascending, &m);
    assert_eq!(ids(&asc), ["e", "b", "d", "a", "c", "f"]);
    let desc = sorted(priced(), "price", SortDirection::Descending, &m);
    assert_eq!(ids(&desc), ["a", "b", "d", "e", "c", "f"]);
}

#[test]
fn without_an_order_the_rows_keep_their_order() {
    let mut rows = priced();
    VistaQuery::new().sort_rows(&mut rows, &schemaless());
    assert_eq!(ids(&rows), ["e", "f", "d", "c", "b", "a"]);
}

#[test]
fn a_dotted_order_column_walks_the_path() {
    let nested = |id: &str, w: i64| (id.to_string(), row(&[("dims", map(&[("w", int(w))]))]));
    let rows = vec![nested("x", 3), nested("y", 1), nested("z", 2)];
    let asc = sorted(rows, "dims.w", SortDirection::Ascending, &schemaless());
    assert_eq!(ids(&asc), ["y", "z", "x"]);
}

/// A sort reads the cell a condition reads: a literal `"a.b"` key stands
/// for the column only when the metadata declares it.
#[test]
fn a_literal_dotted_key_sorts_only_when_declared() {
    let both = |id: &str, flat: i64, nested: i64| {
        let cells = [("a.b", int(flat)), ("a", map(&[("b", int(nested))]))];
        (id.to_string(), row(&cells))
    };
    let rows = vec![both("x", 1, 2), both("y", 2, 1)];
    let by_path = sorted(rows.clone(), "a.b", SortDirection::Ascending, &schemaless());
    assert_eq!(ids(&by_path), ["y", "x"]);
    let declared = VistaMetadata::new().with_column(Column::new("a.b", "int"));
    let by_key = sorted(rows, "a.b", SortDirection::Ascending, &declared);
    assert_eq!(ids(&by_key), ["x", "y"]);
}
