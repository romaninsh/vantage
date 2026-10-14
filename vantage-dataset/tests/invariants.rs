use indexmap::IndexMap;
use serde_json::{Value, json};

use vantage_dataset::invariants::{conform, validate};
use vantage_types::Record;

fn parent_is_p1() -> IndexMap<String, Value> {
    IndexMap::from([("parent".to_string(), json!("p1"))])
}

#[test]
fn conform_fills_an_absent_column() {
    let mut record = Record::from(json!({"name": "a"}));

    conform(&mut record, &parent_is_p1()).unwrap();

    assert_eq!(record["parent"], json!("p1"));
    assert_eq!(record["name"], json!("a"));
}

#[test]
fn conform_fills_a_null_column() {
    let mut record = Record::from(json!({"parent": null}));

    conform(&mut record, &parent_is_p1()).unwrap();

    assert_eq!(record["parent"], json!("p1"));
}

#[test]
fn conform_keeps_an_equal_value() {
    let mut record = Record::from(json!({"parent": "p1", "name": "a"}));

    conform(&mut record, &parent_is_p1()).unwrap();

    assert_eq!(record["parent"], json!("p1"));
    assert_eq!(record["name"], json!("a"));
}

#[test]
fn conform_rejects_a_different_value_with_conflict() {
    let mut record = Record::from(json!({"parent": "p2"}));

    let err = conform(&mut record, &parent_is_p1()).unwrap_err();

    assert!(err.is_conflict(), "{err}");
}

#[test]
fn conform_with_no_invariants_changes_nothing() {
    let mut record = Record::from(json!({"name": "a"}));

    conform(&mut record, &IndexMap::new()).unwrap();

    assert_eq!(record.len(), 1);
}

#[test]
fn conform_checks_every_invariant() {
    let invariants = IndexMap::from([
        ("parent".to_string(), json!("p1")),
        ("kind".to_string(), json!("x")),
    ]);
    let mut record = Record::from(json!({"parent": "p1"}));

    conform(&mut record, &invariants).unwrap();
    assert_eq!(record["kind"], json!("x"));

    let mut clash = Record::from(json!({"parent": "p1", "kind": "y"}));
    assert!(conform(&mut clash, &invariants).unwrap_err().is_conflict());
}

#[test]
fn validate_leaves_an_absent_column_absent() {
    let partial = Record::from(json!({"name": "a"}));

    validate(&partial, &parent_is_p1()).unwrap();

    assert!(partial.get("parent").is_none());
}

#[test]
fn validate_accepts_an_equal_value() {
    validate(&Record::from(json!({"parent": "p1"})), &parent_is_p1()).unwrap();
}

#[test]
fn validate_rejects_a_different_value_with_conflict() {
    let err = validate(&Record::from(json!({"parent": "p2"})), &parent_is_p1()).unwrap_err();

    assert!(err.is_conflict(), "{err}");
}

#[test]
fn validate_rejects_null_with_conflict() {
    let err = validate(&Record::from(json!({"parent": null})), &parent_is_p1()).unwrap_err();

    assert!(err.is_conflict(), "{err}");
}
