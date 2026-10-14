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

/// Twenty rows in a scrambled order: every third lacks the price, every
/// seventh holds a null, and the rest share five prices, ints and floats.
fn scrambled() -> Rows {
    (0..20)
        .map(|n| (n * 7) % 20)
        .map(|n| {
            let id = format!("r{n:02}");
            let cells = match n {
                n if n % 3 == 0 => vec![],
                n if n % 7 == 0 => vec![("price", CborValue::Null)],
                n if n % 2 == 0 => vec![("price", int(n % 5))],
                n => vec![("price", CborValue::Float((n % 5) as f64))],
            };
            (id, row(&cells))
        })
        .collect()
}

#[test]
fn cmp_rows_agrees_with_sort_rows() {
    let m = schemaless();
    for dir in [SortDirection::Ascending, SortDirection::Descending] {
        let query = VistaQuery::new().with_order("price", dir);
        let mut rows = scrambled();
        query.sort_rows(&mut rows, &m);
        for pair in rows.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let ordering = query.cmp_rows((&a.0, &a.1), (&b.0, &b.1), &m);
            assert_eq!(ordering, Ordering::Less, "{dir:?}: {} before {}", a.0, b.0);
        }
    }
    let unordered = VistaQuery::new();
    let (a, b) = (&scrambled()[0], &scrambled()[1]);
    assert_eq!(
        unordered.cmp_rows((&a.0, &a.1), (&b.0, &b.1), &m),
        a.0.cmp(&b.0)
    );
    assert_eq!(
        unordered.cmp_rows((&a.0, &a.1), (&a.0, &a.1), &m),
        Ordering::Equal
    );
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
