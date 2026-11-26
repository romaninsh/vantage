use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::rhai::Handle;
use vantage_vista::{Column, VistaMetadata};

use super::support::{read_write, run, store};

/// Rows `yes`, `no` and `none` of a column `t` holding true, false and NULL.
fn rows() -> Vec<(&'static str, Record<CborValue>)> {
    [
        ("yes", CborValue::Bool(true)),
        ("no", CborValue::Bool(false)),
        ("none", CborValue::Null),
    ]
    .into_iter()
    .map(|(id, t)| (id, [("t".to_string(), t)].into_iter().collect()))
    .collect()
}

/// The ids of [`rows`] the conditions of `script`'s handle match.
fn matching(script: &str) -> Vec<&'static str> {
    let host = read_write(&store());
    let query = run(&host, script)
        .unwrap()
        .try_cast::<Handle>()
        .expect("a table handle")
        .query()
        .unwrap();
    let metadata = VistaMetadata::new().with_column(Column::new("t", "bool"));
    rows()
        .into_iter()
        .filter(|(_, row)| query.matches(row, &metadata).unwrap())
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn is_null_matches_null_row() {
    assert_eq!(matching(r#"table("t").where("t", "is", null())"#), ["none"]);
    assert_eq!(
        matching(r#"table("t").where("t", "is_not", null())"#),
        ["yes", "no"]
    );
    assert_eq!(matching(r#"table("t").where("t", "is", true)"#), ["yes"]);
}

#[test]
fn is_not_true_matches_false_and_null() {
    assert_eq!(
        matching(r#"table("t").where("t", "is_not", true)"#),
        ["no", "none"]
    );
}

#[test]
fn ne_true_matches_only_false() {
    assert_eq!(matching(r#"table("t").where("t", "ne", true)"#), ["no"]);
}

#[test]
fn unit_value_is_refused_with_column_name() {
    let host = read_write(&store());
    for script in [
        r#"table("t").where("t", ())"#,
        r#"table("t").where("t", "is", ())"#,
        r#"let session = #{}; table("t").where("t", session.uid)"#,
    ] {
        let err = run(&host, script).unwrap_err();
        assert!(
            err.contains("where(t): the value is empty (()); use null() to match NULL"),
            "{script}: {err}"
        );
    }
}

#[test]
fn ordering_null_is_refused() {
    let host = read_write(&store());
    for op in ["gt", ">=", "lt", "lte"] {
        let err = run(&host, &format!(r#"table("t").where("t", "{op}", null())"#)).unwrap_err();
        assert!(
            err.contains(r#"where(t): null() can't be ordered; use "is" or "is_not""#),
            "{op}: {err}"
        );
    }
    for script in [
        r#"table("t").where("t", null())"#,
        r#"table("t").where("t", "ne", null())"#,
        r#"table("t").where("t", "!", null())"#,
    ] {
        let err = run(&host, script).unwrap_err();
        assert!(
            err.contains(r#"where(t): null() needs "is" or "is_not""#),
            "{script}: {err}"
        );
    }
}

#[test]
fn null_value_writes_null() {
    let store = store();
    let _ = run(&read_write(&store), r#"table("t").patch("r1", #{ a: null() })"#).unwrap();
    let row = store.table("t").get("r1").unwrap();
    assert_eq!(row.get("a"), Some(&CborValue::Null));
}
