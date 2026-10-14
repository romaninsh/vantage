use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_vista::{Condition, FilterOp, SortDirection, VistaQuery, operand_text};

use super::{i, t};

fn key_of(condition: Condition) -> String {
    VistaQuery::new().with_condition(condition).key()
}

#[test]
fn the_same_clauses_give_the_same_key_whatever_the_condition_order() {
    let a = Condition::new("a", FilterOp::Eq, 1_i64);
    let b = Condition::new("b", FilterOp::Gt, 2_i64);
    let forward = VistaQuery::new()
        .with_condition(a.clone())
        .with_condition(b.clone())
        .with_search("x")
        .with_order("a", SortDirection::Ascending);
    let backward = VistaQuery::new()
        .with_order("a", SortDirection::Ascending)
        .with_search("x")
        .with_condition(b)
        .with_condition(a);
    assert_eq!(forward.key(), backward.key());
}

#[test]
fn native_condition_keys_are_order_independent_and_distinguishing() {
    let mut one = VistaQuery::new();
    one.native = vec!["x = 1".into(), "y = 2".into()];
    let mut other = VistaQuery::new();
    other.native = vec!["y = 2".into(), "x = 1".into()];
    assert_eq!(one.key(), other.key());

    let mut different = VistaQuery::new();
    different.native = vec!["x = 1".into()];
    assert_ne!(one.key(), different.key());
    assert_ne!(different.key(), VistaQuery::new().key());
}

#[test]
fn every_clause_changes_the_key() {
    let empty = VistaQuery::new().key();
    let keys = [
        VistaQuery::new()
            .with_condition(Condition::new("a", FilterOp::Eq, 1_i64))
            .key(),
        VistaQuery::new().with_search("a").key(),
        VistaQuery::new()
            .with_order("a", SortDirection::Ascending)
            .key(),
        VistaQuery::new()
            .with_order("a", SortDirection::Descending)
            .key(),
        empty.clone(),
    ];
    for (n, key) in keys.iter().enumerate() {
        for other in &keys[n + 1..] {
            assert_ne!(key, other);
        }
    }
}

#[test]
fn every_operator_has_its_own_key() {
    let ops = [
        FilterOp::Eq,
        FilterOp::Ne,
        FilterOp::Gt,
        FilterOp::Gte,
        FilterOp::Lt,
        FilterOp::Lte,
        FilterOp::InSet,
        FilterOp::NotInSet,
        FilterOp::Like,
    ];
    let keys: Vec<String> = ops
        .iter()
        .map(|op| key_of(Condition::new("a", *op, 1_i64)))
        .collect();
    for (n, key) in keys.iter().enumerate() {
        for other in &keys[n + 1..] {
            assert_ne!(key, other);
        }
    }
}

#[test]
fn values_of_different_types_never_share_a_key() {
    let values = [
        t("1"),
        i(1),
        CborValue::Float(1.0),
        CborValue::Bool(true),
        CborValue::Null,
        CborValue::Bytes(vec![1]),
        CborValue::Array(vec![i(1)]),
        CborValue::Map(vec![(t("1"), i(1))]),
        CborValue::Tag(0, Box::new(t("1"))),
        CborValue::Tag(1, Box::new(t("1"))),
        CborValue::Array(vec![]),
        CborValue::Map(vec![]),
    ];
    let keys: Vec<String> = values
        .iter()
        .map(|v| key_of(Condition::new("a", FilterOp::Eq, v.clone())))
        .collect();
    for (n, key) in keys.iter().enumerate() {
        for other in &keys[n + 1..] {
            assert_ne!(key, other);
        }
    }
}

#[test]
fn map_entry_order_does_not_change_the_key() {
    let ab = CborValue::Map(vec![(t("a"), i(1)), (t("b"), i(2))]);
    let ba = CborValue::Map(vec![(t("b"), i(2)), (t("a"), i(1))]);
    assert_eq!(
        key_of(Condition::new("m", FilterOp::Eq, ab)),
        key_of(Condition::new("m", FilterOp::Eq, ba))
    );
}

#[test]
fn separators_in_names_and_values_do_not_collide() {
    let split = |column: &str, value: &str| key_of(Condition::new(column, FilterOp::Eq, t(value)));
    assert_ne!(split("a:b", "c"), split("a", "b:c"));
    assert_ne!(split("a,b", "c"), split("a", "b,c"));
    assert_ne!(split("a=b", "c"), split("a", "b=c"));

    let two = VistaQuery::new()
        .with_condition(Condition::new("a", FilterOp::Eq, t("x")))
        .with_condition(Condition::new("b", FilterOp::Eq, t("y")))
        .key();
    let one = VistaQuery::new()
        .with_condition(Condition::new("a", FilterOp::Eq, t("x&b=y")))
        .key();
    assert_ne!(two, one);
    assert_ne!(
        VistaQuery::new().with_search("a|b").key(),
        VistaQuery::new()
            .with_search("a")
            .with_order("b", SortDirection::Ascending)
            .key()
    );
}

#[test]
fn an_empty_query_is_empty_and_any_clause_makes_it_not() {
    assert!(VistaQuery::new().is_empty());
    assert!(
        !VistaQuery::new()
            .with_order("a", SortDirection::Ascending)
            .is_empty()
    );
    assert!(!VistaQuery::new().with_search("x").is_empty());
}

#[test]
fn operators_parse_from_symbols_and_words() {
    assert_eq!(FilterOp::parse(">="), Some(FilterOp::Gte));
    assert_eq!(FilterOp::parse("gte"), Some(FilterOp::Gte));
    assert_eq!(FilterOp::parse("!="), Some(FilterOp::Ne));
    assert_eq!(FilterOp::parse("ne"), Some(FilterOp::Ne));
    assert_eq!(FilterOp::parse("~"), Some(FilterOp::Like));
    assert_eq!(FilterOp::parse("like"), Some(FilterOp::Like));
    assert_eq!(FilterOp::parse(">"), Some(FilterOp::Gt));
    assert_eq!(FilterOp::parse("<"), Some(FilterOp::Lt));
    assert_eq!(FilterOp::parse("<="), Some(FilterOp::Lte));
    assert_eq!(FilterOp::parse("eq"), Some(FilterOp::Eq));
    assert_eq!(FilterOp::parse("nonsense"), None);
}

#[test]
fn a_typed_prefix_splits_into_operator_and_value() {
    assert_eq!(FilterOp::split_prefix(">30"), (FilterOp::Gt, "30"));
    assert_eq!(FilterOp::split_prefix("~smith"), (FilterOp::Like, "smith"));
    assert_eq!(FilterOp::split_prefix("30"), (FilterOp::Eq, "30"));
}

#[test]
fn equality_displays_as_nothing_and_set_operators_take_sets() {
    assert_eq!(FilterOp::Eq.display_symbol(), "");
    assert!(!FilterOp::Gte.display_symbol().is_empty());
    assert_eq!(FilterOp::Gt.display_symbol(), ">");
    assert_eq!(FilterOp::Like.display_symbol(), "~");
    assert!(FilterOp::InSet.takes_set());
    assert!(FilterOp::NotInSet.takes_set());
    assert!(!FilterOp::Eq.takes_set());
}

#[test]
fn ordered_operators_read_the_ordering_of_cell_against_operand() {
    assert!(FilterOp::Gt.matches_ordering(Ordering::Greater));
    assert!(!FilterOp::Gt.matches_ordering(Ordering::Equal));
    assert!(FilterOp::Gte.matches_ordering(Ordering::Equal));
    assert!(!FilterOp::Gte.matches_ordering(Ordering::Less));
    assert!(FilterOp::Lt.matches_ordering(Ordering::Less));
    assert!(!FilterOp::Lt.matches_ordering(Ordering::Equal));
    assert!(FilterOp::Lte.matches_ordering(Ordering::Equal));
    assert!(!FilterOp::Lte.matches_ordering(Ordering::Greater));
}

#[test]
fn a_like_operand_reads_as_its_text() {
    assert_eq!(operand_text(&t("smith")), "smith");
    assert_eq!(operand_text(&i(42)), "42");
    assert_eq!(operand_text(&CborValue::Bool(true)), "true");
    let record_id = CborValue::Tag(
        8,
        Box::new(CborValue::Array(vec![t("client"), t("bristol")])),
    );
    assert_eq!(operand_text(&record_id), "client:bristol");
}
