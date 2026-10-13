use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::{cbor_cmp, cell_get, path_get};
use crate::{Column, Condition, FilterOp, VistaMetadata, VistaQuery, filter::operand_text};

mod search;
mod sort;

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn map(pairs: &[(&str, CborValue)]) -> CborValue {
    CborValue::Map(pairs.iter().map(|(k, v)| (text(k), v.clone())).collect())
}

fn row(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn schemaless() -> VistaMetadata {
    VistaMetadata::new()
}

/// `n == 5` and `s == "Straße"`, flat and nested under `dims`.
fn sample() -> Record<CborValue> {
    row(&[
        ("n", int(5)),
        ("s", text("Straße")),
        ("dims", map(&[("n", int(5)), ("s", text("Straße"))])),
    ])
}

/// Every operator against `column`, which holds 5, and `text_column`,
/// which holds "Straße".
fn assert_every_operator(column: &str, text_column: &str) {
    let r = sample();
    let m = schemaless();
    let n = |op, v: i64| Condition::new(column, op, v).matches(&r, &m);
    assert!(n(FilterOp::Eq, 5) && !n(FilterOp::Eq, 4), "{column}: eq");
    assert!(n(FilterOp::Ne, 4) && !n(FilterOp::Ne, 5), "{column}: ne");
    assert!(n(FilterOp::Gt, 4) && !n(FilterOp::Gt, 5), "{column}: gt");
    assert!(n(FilterOp::Gte, 5) && !n(FilterOp::Gte, 6), "{column}: gte");
    assert!(n(FilterOp::Lt, 6) && !n(FilterOp::Lt, 5), "{column}: lt");
    assert!(n(FilterOp::Lte, 5) && !n(FilterOp::Lte, 4), "{column}: lte");
    let set = |items: Vec<CborValue>| CborValue::Array(items);
    let c = |op, v: CborValue| Condition::new(column, op, v).matches(&r, &m);
    assert!(
        c(FilterOp::InSet, set(vec![int(1), int(5)])),
        "{column}: in"
    );
    assert!(!c(FilterOp::InSet, set(vec![int(1)])), "{column}: in");
    assert!(c(FilterOp::NotInSet, set(vec![int(1)])), "{column}: not in");
    assert!(
        !c(FilterOp::NotInSet, set(vec![int(5)])),
        "{column}: not in"
    );
    let like = |v: &str| Condition::new(text_column, FilterOp::Like, v).matches(&r, &m);
    assert!(like("STRA") && !like("strasse"), "{text_column}: like");
}

#[test]
fn every_operator_evaluates_over_a_flat_cell() {
    assert_every_operator("n", "s");
}

#[test]
fn every_operator_evaluates_over_a_nested_cell() {
    assert_every_operator("dims.n", "dims.s");
}

#[test]
fn an_absent_cell_matches_only_the_negative_operators() {
    let r = sample();
    let m = schemaless();
    for column in ["missing", "dims.missing", "n.deeper"] {
        let matches = |op, v: CborValue| Condition::new(column, op, v).matches(&r, &m);
        assert!(matches(FilterOp::Ne, int(1)), "{column}");
        assert!(matches(FilterOp::NotInSet, CborValue::Array(vec![int(1)])));
        for op in [FilterOp::Eq, FilterOp::Gt, FilterOp::Lte, FilterOp::Like] {
            assert!(!matches(op, int(1)), "{column} {op:?}");
        }
        assert!(!matches(FilterOp::InSet, CborValue::Array(vec![int(1)])));
    }
}

#[test]
fn integers_and_floats_compare_numerically() {
    let int_row = row(&[("price", int(3))]);
    let float_row = row(&[("price", CborValue::Float(4.95))]);
    let m = schemaless();
    let p = |op, v: CborValue, r: &Record<CborValue>| Condition::new("price", op, v).matches(r, &m);
    assert!(p(FilterOp::Lt, CborValue::Float(4.9), &int_row));
    assert!(!p(FilterOp::Lt, CborValue::Float(4.9), &float_row));
    assert!(p(FilterOp::Gt, int(4), &float_row));
    assert!(!p(FilterOp::Gt, CborValue::Float(3.5), &int_row));
    assert!(p(FilterOp::Gte, CborValue::Float(3.0), &int_row));
    assert!(p(FilterOp::Eq, CborValue::Float(3.0), &int_row));
    assert!(!p(FilterOp::Ne, CborValue::Float(3.0), &int_row));
    let set = CborValue::Array(vec![CborValue::Float(3.0)]);
    assert!(p(FilterOp::InSet, set, &int_row));
    assert!(!p(FilterOp::Eq, text("3"), &int_row));
    assert_eq!(cbor_cmp(&int(3), &CborValue::Float(4.95)), Ordering::Less);
}

/// Mixed types with no common order still compare, and the same way
/// every time.
#[test]
fn mixed_types_compare_deterministically() {
    let (a, b) = (text("5"), int(5));
    assert_eq!(cbor_cmp(&a, &b), cbor_cmp(&a, &b));
    assert_eq!(cbor_cmp(&a, &b), cbor_cmp(&b, &a).reverse());
    assert_ne!(cbor_cmp(&a, &b), Ordering::Equal);
    assert_eq!(
        cbor_cmp(&CborValue::Bool(false), &CborValue::Bool(true)),
        Ordering::Less
    );
}

/// A literal dotted key is a flat cell only when the metadata declares it;
/// otherwise the path is walked, so the literal key can't stand in for it.
#[test]
fn a_literal_dotted_key_is_read_only_when_declared() {
    let r = row(&[("a.b", int(1)), ("a", map(&[("b", int(2))]))]);
    let declared = VistaMetadata::new().with_column(Column::new("a.b", "int"));
    assert_eq!(cell_get(&r, "a.b", &declared), Some(&int(1)));
    assert_eq!(cell_get(&r, "a.b", &schemaless()), Some(&int(2)));
    assert_eq!(path_get(&r, "a.b"), Some(&int(2)));
    assert_eq!(path_get(&r, "a"), r.get("a"));
    assert_eq!(path_get(&r, "a.b.c"), None);
}

#[test]
fn a_query_matches_when_every_condition_and_the_search_do() {
    let r = sample();
    let m = schemaless();
    let query = VistaQuery::new()
        .with_condition(Condition::new("n", FilterOp::Gte, 5))
        .with_condition(Condition::new("dims.s", FilterOp::Eq, "Straße"))
        .with_search("str");
    assert!(query.matches(&r, &m).unwrap());
    let other = query.with_condition(Condition::new("n", FilterOp::Lt, 5));
    assert!(!other.matches(&r, &m).unwrap());
}

#[test]
fn a_query_holding_a_native_condition_is_not_evaluated() {
    let mut query = VistaQuery::new();
    assert!(query.is_locally_evaluable());
    query.native.push("mongo:{}".to_string());
    assert!(!query.is_locally_evaluable());
    let err = query.matches(&sample(), &schemaless()).unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
}

/// `-0.0` is taken as `0.0`: equal to `0` and `0.0`, and not below either.
#[test]
fn negative_zero_is_zero() {
    let r = row(&[("t", CborValue::Float(-0.0))]);
    let m = schemaless();
    let t = |op, v: CborValue| Condition::new("t", op, v).matches(&r, &m);
    assert!(t(FilterOp::Eq, int(0)) && t(FilterOp::Eq, CborValue::Float(0.0)));
    assert!(!t(FilterOp::Lt, int(0)) && !t(FilterOp::Lt, CborValue::Float(0.0)));
    let nan = CborValue::Float(f64::NAN);
    assert_eq!(cbor_cmp(&nan, &nan), Ordering::Equal);
    assert_eq!(
        cbor_cmp(&nan, &CborValue::Float(f64::INFINITY)),
        Ordering::Greater
    );
}

#[test]
fn like_compares_the_operand_as_text() {
    let r = row(&[("code", int(1234))]);
    let like = Condition::new("code", FilterOp::Like, "23");
    assert!(like.matches(&r, &schemaless()));
    assert_eq!(operand_text(&int(1234)), "1234");
}
