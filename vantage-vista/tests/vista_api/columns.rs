use std::any::type_name;

use vantage_vista::{
    Capability, Column, ColumnFamily, ColumnFlag, VistaCapabilities, unknown_relation, unsupported,
};

fn family(t: &str) -> Option<ColumnFamily> {
    Column::new("c", t).family()
}

#[test]
fn rust_type_names_classify_like_their_short_names() {
    assert_eq!(family(type_name::<String>()), Some(ColumnFamily::Text));
    assert_eq!(family(type_name::<&str>()), Some(ColumnFamily::Text));
    assert_eq!(
        family(type_name::<Option<i64>>()),
        Some(ColumnFamily::Integer)
    );
    assert_eq!(
        family(type_name::<Option<String>>()),
        Some(ColumnFamily::Text)
    );
    assert_eq!(family(type_name::<bool>()), Some(ColumnFamily::Bool));
    assert_eq!(family(type_name::<f64>()), Some(ColumnFamily::Float));
    assert_eq!(
        family("rust_decimal::decimal::Decimal"),
        Some(ColumnFamily::Decimal)
    );
}

#[test]
fn spec_and_sql_type_names_classify_case_insensitively() {
    assert_eq!(family("BIGINT"), Some(ColumnFamily::Integer));
    assert_eq!(family("varchar(255)"), Some(ColumnFamily::Text));
    assert_eq!(family("numeric(10, 2)"), Some(ColumnFamily::Decimal));
    assert_eq!(family("Boolean"), Some(ColumnFamily::Bool));
    assert_eq!(family("jsonb"), None);
    assert_eq!(family(type_name::<Vec<u8>>()), None);
    assert_eq!(Column::untyped("c").family(), None);
}

#[test]
fn an_untyped_column_serializes_without_a_type() {
    let json = serde_json::to_value(Column::untyped("note")).unwrap();
    assert_eq!(json, serde_json::json!({ "name": "note" }));
    let back: Column = serde_json::from_value(json).unwrap();
    assert_eq!(back.original_type, None);

    let typed = serde_json::to_value(Column::new("n", "int")).unwrap();
    assert_eq!(typed["original_type"], "int");
}

#[test]
fn flags_are_snake_case_names_in_yaml() {
    let flags: Vec<ColumnFlag> =
        serde_yaml_ng::from_str("[id, title, searchable, orderable, mandatory, hidden, request_param, calculated, auto, label]")
            .unwrap();
    assert_eq!(
        flags,
        [
            ColumnFlag::Id,
            ColumnFlag::Title,
            ColumnFlag::Searchable,
            ColumnFlag::Orderable,
            ColumnFlag::Mandatory,
            ColumnFlag::Hidden,
            ColumnFlag::RequestParam,
            ColumnFlag::Calculated,
            ColumnFlag::Auto,
            ColumnFlag::Other("label".into())
        ]
    );
    assert_eq!(
        serde_yaml_ng::to_string(&flags).unwrap(),
        "- id\n- title\n- searchable\n- orderable\n- mandatory\n- hidden\n- request_param\n- calculated\n- auto\n- label\n"
    );
    for flag in &flags {
        assert_eq!(ColumnFlag::from(flag.as_str()), *flag);
    }
}

#[test]
fn a_hand_built_other_with_a_known_name_is_that_flag() {
    assert_eq!(ColumnFlag::Other("id".into()), ColumnFlag::Id);
    let column = Column::new("a", "int").with_flag(ColumnFlag::Other("hidden".into()));
    assert!(column.has_flag(ColumnFlag::Hidden));
    assert!(matches!(ColumnFlag::from("hidden"), ColumnFlag::Hidden));
    let twice = column.with_flag(ColumnFlag::Hidden);
    assert_eq!(twice.flags().len(), 1);
}

#[test]
fn the_capability_vocabulary_is_listed_once_and_round_trips_by_name() {
    let all: Vec<Capability> = Capability::iter().collect();
    assert_eq!(all.len(), 13);
    let mut names: Vec<&str> = all.iter().map(|c| c.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all.len(), "names are distinct");
    for capability in all {
        let json = serde_json::to_value(capability).unwrap();
        assert_eq!(json, capability.as_str());
        let back: Capability = serde_json::from_value(json).unwrap();
        assert_eq!(back, capability);
        assert_eq!(capability.to_string(), capability.as_str());
    }
}

#[test]
fn an_unknown_capability_name_is_rejected() {
    let err = serde_json::from_value::<Capability>(serde_json::json!("teleport")).unwrap_err();
    assert!(err.to_string().contains("teleport"), "{err}");
    let err = serde_json::from_value::<VistaCapabilities>(serde_json::json!({ "teleport": true }))
        .unwrap_err();
    assert!(err.to_string().contains("teleport"), "{err}");
}

#[test]
fn capability_sets_add_remove_and_serialize_every_name() {
    let set = VistaCapabilities::new()
        .with(Capability::Insert)
        .with(Capability::Window)
        .with(Capability::Insert);
    assert!(set.has(Capability::Insert) && set.has(Capability::Window));
    assert!(!set.has(Capability::Delete));
    assert_eq!(
        set.iter().collect::<Vec<_>>(),
        [Capability::Insert, Capability::Window]
    );
    let without = set.without(Capability::Insert);
    assert!(!without.has(Capability::Insert));

    let json = serde_json::to_value(set).unwrap();
    let map = json.as_object().unwrap();
    assert_eq!(map.len(), 13);
    assert_eq!(map["insert"], true);
    assert_eq!(map["delete"], false);
    let back: VistaCapabilities = serde_json::from_value(json).unwrap();
    assert_eq!(back, set);

    let collected: VistaCapabilities = [Capability::Order, Capability::Search]
        .into_iter()
        .collect();
    assert!(collected.has(Capability::Order) && collected.has(Capability::Search));
    assert_eq!(format!("{collected:?}"), "{Order, Search}");
}

#[test]
fn a_refused_capability_is_marked_unsupported_and_names_it() {
    let err = unsupported::<()>(Capability::Cursor).unwrap_err();
    assert!(err.is_unsupported());
    assert!(format!("{err:?}").contains("cursor") || err.to_string().contains("cursor"));
}

#[test]
fn an_unknown_relation_is_not_found_and_names_the_relation() {
    let err = unknown_relation("orders");
    assert!(err.is_not_found());
    assert!(format!("{err:?}").contains("orders") || err.to_string().contains("orders"));
}
