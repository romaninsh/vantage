use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{FilterOp, SortDirection};

use super::*;
use crate::store::MemoryStore;

fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}
fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
fn yes(c: MemoryCondition, r: &Record<CborValue>) -> bool {
    c.matches(r).unwrap()
}

#[test]
fn ordered_ops_compare_numbers_across_int_and_float() {
    let r = rec(&[("n", int(5))]);
    assert!(yes(
        MemoryCondition::cmp("n", FilterOp::Gt, CborValue::Float(4.5)),
        &r
    ));
    assert!(yes(MemoryCondition::cmp("n", FilterOp::Gte, int(5)), &r));
    assert!(yes(
        MemoryCondition::cmp("n", FilterOp::Lt, CborValue::Float(5.1)),
        &r
    ));
    assert!(!yes(MemoryCondition::cmp("n", FilterOp::Lte, int(4)), &r));
}

#[test]
fn eq_int_matches_float() {
    let r = rec(&[("n", CborValue::Float(1.0))]);
    assert!(yes(MemoryCondition::cmp("n", FilterOp::Eq, int(1)), &r));
    assert!(yes(
        MemoryCondition::cmp("n", FilterOp::InSet, CborValue::Array(vec![int(3), int(1)])),
        &r
    ));
}

#[test]
fn mixed_kinds_never_match_ordered_ops() {
    let r = rec(&[("n", text("5"))]);
    assert!(!yes(MemoryCondition::cmp("n", FilterOp::Gt, int(1)), &r));
    assert!(!yes(MemoryCondition::cmp("n", FilterOp::Lt, int(9)), &r));
}

#[test]
fn null_rules() {
    let r = rec(&[("a", CborValue::Null)]);
    let missing = rec(&[]);
    assert!(yes(
        MemoryCondition::cmp("a", FilterOp::Eq, CborValue::Null),
        &r
    ));
    assert!(yes(
        MemoryCondition::cmp("a", FilterOp::Eq, CborValue::Null),
        &missing
    ));
    assert!(yes(MemoryCondition::cmp("a", FilterOp::Ne, int(1)), &r));
    assert!(!yes(
        MemoryCondition::cmp("a", FilterOp::Ne, CborValue::Null),
        &r
    ));
    assert!(yes(
        MemoryCondition::cmp("a", FilterOp::NotInSet, CborValue::Array(vec![int(1)])),
        &r
    ));
    assert!(!yes(
        MemoryCondition::cmp(
            "a",
            FilterOp::NotInSet,
            CborValue::Array(vec![CborValue::Null])
        ),
        &r
    ));
    assert!(!yes(MemoryCondition::cmp("a", FilterOp::Gt, int(0)), &r));
    assert!(!yes(
        MemoryCondition::cmp("a", FilterOp::Like, text("%")),
        &r
    ));
}

#[test]
fn dotted_paths_read_nested_maps() {
    let addr = CborValue::Map(vec![(text("city"), text("Riga"))]);
    let r = rec(&[("address", addr)]);
    assert!(yes(
        MemoryCondition::cmp("address.city", FilterOp::Eq, text("Riga")),
        &r
    ));
    assert!(yes(
        MemoryCondition::cmp("address.zip", FilterOp::Eq, CborValue::Null),
        &r
    ));
}

#[test]
fn a_column_named_with_a_dot_wins_over_the_path() {
    let r = rec(&[("a.b", int(1))]);
    assert!(yes(MemoryCondition::cmp("a.b", FilterOp::Eq, int(1)), &r));
}

#[test]
fn like_is_case_insensitive_with_wildcards() {
    let r = rec(&[("name", text("Hello World"))]);
    assert!(yes(
        MemoryCondition::cmp("name", FilterOp::Like, text("hello%")),
        &r
    ));
    assert!(yes(
        MemoryCondition::cmp("name", FilterOp::Like, text("%o_w%")),
        &r
    ));
    assert!(!yes(
        MemoryCondition::cmp("name", FilterOp::Like, text("world")),
        &r
    ));
    let n = rec(&[("n", int(12345))]);
    assert!(yes(
        MemoryCondition::cmp("n", FilterOp::Like, text("%234%")),
        &n
    ));
}

#[test]
fn boolean_combinators() {
    let r = rec(&[("a", int(1)), ("b", int(2))]);
    let a1 = MemoryCondition::cmp("a", FilterOp::Eq, int(1));
    let b9 = MemoryCondition::cmp("b", FilterOp::Eq, int(9));
    assert!(!yes(MemoryCondition::And(vec![a1.clone(), b9.clone()]), &r));
    assert!(yes(MemoryCondition::Or(vec![a1.clone(), b9.clone()]), &r));
    assert!(yes(MemoryCondition::Not(Box::new(b9)), &r));
}

#[test]
fn search_checks_text_and_numbers_case_insensitively() {
    let r = rec(&[("name", text("Alice")), ("n", int(42))]);
    assert!(yes(MemoryCondition::Search("ALI".into()), &r));
    assert!(yes(MemoryCondition::Search("42".into()), &r));
    assert!(!yes(MemoryCondition::Search("bob".into()), &r));
}

#[test]
fn column_and_unresolved_deferred_are_errors_as_filters() {
    let r = rec(&[]);
    assert!(MemoryCondition::Column("a".into()).matches(&r).is_err());
}

#[tokio::test]
async fn deferred_resolves_to_in_set() {
    use vantage_expressions::traits::expressive::DeferredFn;
    let d = DeferredFn::from_fn(|| async {
        Ok::<_, vantage_core::VantageError>(crate::AnyMemoryType::untyped(CborValue::Array(vec![
            text("owner"),
            CborValue::Array(vec![text("u1"), text("u2")]),
        ])))
    });
    let c = MemoryCondition::And(vec![MemoryCondition::Deferred(d)])
        .resolve()
        .await
        .unwrap();
    assert!(yes(c.clone(), &rec(&[("owner", text("u2"))])));
    assert!(!yes(c, &rec(&[("owner", text("u3"))])));
}

fn filled() -> crate::store::MemoryTableHandle {
    let t = MemoryStore::new().table("t");
    for (id, n, name) in [
        ("a", 3, "Cy"),
        ("b", 1, "Al"),
        ("c", 2, "Bo"),
        ("d", 1, "Di"),
    ] {
        t.upsert(id, rec(&[("n", int(n)), ("name", text(name))]));
    }
    t
}

fn ids(rows: Vec<(String, crate::Row)>) -> Vec<String> {
    rows.into_iter().map(|(id, _)| id).collect()
}

#[test]
fn query_filters_orders_and_pages() {
    let t = filled();
    let q = Query::new()
        .filter(MemoryCondition::cmp("n", FilterOp::Lte, int(2)))
        .order_by("n", SortDirection::Ascending)
        .order_by("name", SortDirection::Descending);
    assert_eq!(ids(t.query(&q).unwrap()), ["d", "b", "c"]);
    assert_eq!(ids(t.query(&q.clone().window(1, Some(1))).unwrap()), ["b"]);
    assert_eq!(t.count(&q).unwrap(), 3);
}

#[test]
fn ordering_ties_keep_insertion_order_and_nulls_first() {
    let t = filled();
    t.upsert("e", rec(&[("name", text("Ed"))]));
    let q = Query::new().order_by("n", SortDirection::Ascending);
    assert_eq!(ids(t.query(&q).unwrap()), ["e", "b", "d", "c", "a"]);
    let q = Query::new().order_by("n", SortDirection::Descending);
    assert_eq!(ids(t.query(&q).unwrap()), ["a", "c", "b", "d", "e"]);
}

#[test]
fn nan_sorts_after_every_number() {
    let t = MemoryStore::new().table("t");
    for (id, n) in [("a", f64::NAN), ("b", 2.0), ("c", f64::NAN), ("d", -1.0)] {
        t.upsert(id, rec(&[("n", CborValue::Float(n))]));
    }
    t.upsert("e", rec(&[("n", int(5))]));
    let q = Query::new().order_by("n", SortDirection::Ascending);
    assert_eq!(ids(t.query(&q).unwrap()), ["d", "b", "e", "a", "c"]);
    let q = Query::new().order_by("n", SortDirection::Descending);
    assert_eq!(ids(t.query(&q).unwrap()), ["a", "c", "e", "b", "d"]);
}

#[test]
fn offset_past_end_is_empty() {
    let t = filled();
    assert!(
        t.query(&Query::new().window(10, Some(5)))
            .unwrap()
            .is_empty()
    );
    assert!(
        t.query(&Query::new().window(0, Some(0)))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn query_search_combines_with_conditions() {
    let t = filled();
    let q = Query::new()
        .search("o")
        .filter(MemoryCondition::cmp("n", FilterOp::Eq, int(2)));
    assert_eq!(ids(t.query(&q).unwrap()), ["c"]);
}
