use super::*;

/// The position of each variant. The match is exhaustive, so a new variant
/// fails to compile here until it gets a position, and the test below then
/// fails until `ALL` lists it.
fn ordinal(c: Capability) -> usize {
    match c {
        Capability::Insert => 0,
        Capability::Update => 1,
        Capability::Delete => 2,
        Capability::Import => 3,
        Capability::Subscribe => 4,
        Capability::Order => 5,
        Capability::Search => 6,
        Capability::FilterOps => 7,
        Capability::NativeConditions => 8,
        Capability::Window => 9,
        Capability::Cursor => 10,
        Capability::Traverse => 11,
        Capability::ConfineWrites => 12,
    }
}

/// The highest position `ordinal` assigns; keep it next to the match.
const LAST: usize = 12;

#[test]
fn all_lists_every_variant_once() {
    let positions: Vec<usize> = ALL.iter().map(|c| ordinal(*c)).collect();
    assert_eq!(positions, (0..=LAST).collect::<Vec<_>>());
    assert!(ALL.len() <= 16, "the set is a u16 bitset");
}

#[test]
fn names_round_trip_and_are_distinct() {
    let names: std::collections::HashSet<_> = Capability::iter().map(|c| c.as_str()).collect();
    assert_eq!(names.len(), ALL.len());
    for c in Capability::iter() {
        assert_eq!(Capability::parse(c.as_str()), Some(c));
    }
}

#[test]
fn set_operations() {
    let set: VistaCapabilities = [Capability::Insert, Capability::Window]
        .into_iter()
        .collect();
    assert!(set.has(Capability::Insert) && set.has(Capability::Window));
    assert!(!set.has(Capability::Delete));
    assert!(!set.without(Capability::Insert).has(Capability::Insert));
    assert_eq!(set.iter().count(), 2);
}

#[test]
fn deserializing_reads_known_names() {
    let set: VistaCapabilities =
        serde_yaml_ng::from_str("{ insert: true, delete: false }").unwrap();
    assert_eq!(set, VistaCapabilities::new().with(Capability::Insert));
}

#[test]
fn deserializing_rejects_unknown_names() {
    let err = serde_yaml_ng::from_str::<VistaCapabilities>("{ insert: true, fetch_next: true }")
        .unwrap_err();
    assert!(
        err.to_string().contains("unknown capability `fetch_next`"),
        "{err}"
    );
}

#[test]
fn unsupported_is_marked_and_names_the_capability() {
    let err = unsupported::<()>(Capability::Cursor).unwrap_err();
    assert!(err.is_unsupported());
    assert!(err.context["capability"].contains("cursor"), "{err}");
}

#[test]
fn unknown_relation_is_not_found_and_names_the_relation() {
    let err = unknown_relation("orders");
    assert!(err.is_not_found());
    assert!(err.context["relation"].contains("orders"), "{err}");
}
