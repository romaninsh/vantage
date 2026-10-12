use ciborium::Value as CborValue;

use super::{Condition, VistaQuery};
use crate::{FilterOp, SortDirection};

fn eq(column: &str, value: impl Into<CborValue>) -> Condition {
    Condition::new(column, FilterOp::Eq, value)
}

fn natives(keys: &[&str]) -> VistaQuery {
    VistaQuery {
        native: keys.iter().map(|k| k.to_string()).collect(),
        ..VistaQuery::default()
    }
}

#[test]
fn same_query_gives_the_same_key_whatever_the_clause_order() {
    let ab = VistaQuery::new()
        .with_condition(eq("branch", "main"))
        .with_condition(eq("status", 1));
    let ba = VistaQuery::new()
        .with_condition(eq("status", 1))
        .with_condition(eq("branch", "main"));
    assert_eq!(ab.key(), ba.key());
    assert_eq!(natives(&["x", "y"]).key(), natives(&["y", "x"]).key());
}

#[test]
fn every_clause_changes_the_key() {
    let base = VistaQuery::new().with_condition(eq("branch", "main"));
    let keys = [
        VistaQuery::new().key(),
        base.key(),
        VistaQuery::new().with_condition(eq("branch", "dev")).key(),
        VistaQuery::new()
            .with_condition(Condition::new("branch", FilterOp::Ne, "main"))
            .key(),
        VistaQuery::new().with_condition(eq("branch", 1)).key(),
        VistaQuery::new().with_condition(eq("branch", "1")).key(),
        base.clone().with_search("x").key(),
        base.clone()
            .with_order("status", SortDirection::Ascending)
            .key(),
        base.clone()
            .with_order("status", SortDirection::Descending)
            .key(),
        natives(&["a"]).key(),
        natives(&["b"]).key(),
    ];
    let distinct: std::collections::HashSet<_> = keys.iter().collect();
    assert_eq!(distinct.len(), keys.len(), "{keys:#?}");
}

/// Shapes that collided when parts were joined by bare separators.
#[test]
fn separators_in_names_and_values_do_not_collide() {
    let pairs = [
        (
            VistaQuery::new()
                .with_condition(eq("a", "x"))
                .with_condition(eq("b", "y")),
            VistaQuery::new().with_condition(eq("a", "x;b=t:y")),
        ),
        (
            VistaQuery::new().with_condition(Condition::new("status", FilterOp::Ne, "x")),
            VistaQuery::new().with_condition(eq("status!", "x")),
        ),
        (
            VistaQuery::new().with_search("a|s:b"),
            VistaQuery::new()
                .with_search("a")
                .with_order("b", SortDirection::Ascending),
        ),
        (natives(&["a", "b"]), natives(&["a;b"])),
        (
            VistaQuery::new().with_condition(eq("a", CborValue::Tag(8, Box::new("x".into())))),
            VistaQuery::new().with_condition(eq("a", "x")),
        ),
    ];
    for (a, b) in pairs {
        assert_ne!(a.key(), b.key(), "{a:?} vs {b:?}");
    }
}

#[test]
fn order_alone_does_not_narrow() {
    let ordered = VistaQuery::new().with_order("a", SortDirection::Ascending);
    assert!(!ordered.is_empty());
    assert!(!ordered.narrows());
    assert!(VistaQuery::new().with_search("x").narrows());
    assert!(natives(&["x"]).narrows());
}
