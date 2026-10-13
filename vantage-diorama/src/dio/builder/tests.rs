use vantage_vista::{Condition, FilterOp, VistaQuery};

use super::cache_identity;

fn red() -> VistaQuery {
    VistaQuery::new().with_condition(Condition::new("team", FilterOp::Eq, "red"))
}

#[test]
fn an_unnarrowed_dio_keeps_the_vista_name() {
    assert_eq!(cache_identity("orders", &VistaQuery::new()), "orders");
}

#[test]
fn a_narrowed_dio_appends_the_query_key() {
    let query = red();
    assert_eq!(
        cache_identity("members", &query),
        format!("members?{}", query.key())
    );
}

#[test]
fn control_characters_and_percent_are_encoded() {
    let query =
        VistaQuery::new().with_condition(Condition::new("note", FilterOp::Eq, "a\u{1}b%c\nd"));
    let identity = cache_identity("t", &query);
    assert!(!identity.chars().any(char::is_control), "{identity:?}");
    assert!(identity.contains("a%01b%25c%0Ad"), "{identity:?}");
}

/// A vista named like another vista's narrowed table gets a table of its own.
#[test]
fn a_name_with_a_question_mark_never_collides_with_a_derived_key() {
    let query = red();
    let lookalike = format!("a?{}", query.key());
    let unnarrowed = cache_identity(&lookalike, &VistaQuery::new());
    assert_ne!(unnarrowed, cache_identity("a", &query));
    assert!(unnarrowed.starts_with("a%3F"), "{unnarrowed:?}");
    assert!(!unnarrowed.contains('?'), "{unnarrowed:?}");
}

#[test]
fn control_characters_in_the_name_are_encoded() {
    assert_eq!(cache_identity("t\u{1}a%b", &VistaQuery::new()), "t%01a%25b");
    let query = red();
    assert_eq!(
        cache_identity("t\u{1}", &query),
        format!("t%01?{}", query.key())
    );
}
