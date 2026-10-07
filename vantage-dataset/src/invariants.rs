//! The set-invariant rule, one place for every layer.
//!
//! A set narrowed by literal `column = value` conditions holds those pairs as
//! invariants. Full-record writes (insert, replace) conform to them; partial
//! writes (patch) are validated against them and never filled.

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::{InvariantValue, Record};

/// Conform `record` to `invariants`, per column: absent → filled; null →
/// filled; equal → kept; different → `Conflict`.
pub fn conform<V: InvariantValue>(
    record: &mut Record<V>,
    invariants: &IndexMap<String, V>,
) -> Result<()> {
    for (column, expected) in invariants {
        match record.get(column) {
            None => {
                record.insert(column.clone(), expected.clone());
            }
            Some(value) if value.is_null() => {
                record.insert(column.clone(), expected.clone());
            }
            Some(value) if value.value_eq(expected) => {}
            Some(_) => {
                return Err(error!(
                    "value conflicts with the set it is written into",
                    column = column.as_str()
                )
                .mark_conflict());
            }
        }
    }
    Ok(())
}

/// Check a partial write against `invariants` without filling anything: an
/// absent column stays absent; a present value must equal the invariant (a
/// null or a different value would move the row out of the set → `Conflict`).
pub fn validate<V: InvariantValue>(
    partial: &Record<V>,
    invariants: &IndexMap<String, V>,
) -> Result<()> {
    for (column, expected) in invariants {
        if let Some(value) = partial.get(column)
            && (value.is_null() || !value.value_eq(expected))
        {
            return Err(error!(
                "patch would move the row out of the set",
                column = column.as_str()
            )
            .mark_conflict());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn inv() -> IndexMap<String, Value> {
        IndexMap::from([("parent".to_string(), json!("p1"))])
    }

    #[test]
    fn conform_fills_keeps_and_rejects() {
        let mut absent = Record::from(json!({"name": "a"}));
        conform(&mut absent, &inv()).unwrap();
        assert_eq!(absent["parent"], json!("p1"));
        let mut null = Record::from(json!({"parent": null}));
        conform(&mut null, &inv()).unwrap();
        assert_eq!(null["parent"], json!("p1"));
        let mut other = Record::from(json!({"parent": "p2"}));
        assert!(conform(&mut other, &inv()).unwrap_err().is_conflict());
    }

    #[test]
    fn validate_never_fills() {
        let absent = Record::from(json!({"name": "a"}));
        validate(&absent, &inv()).unwrap();
        assert!(absent.get("parent").is_none());
        assert!(
            validate(&Record::from(json!({"parent": "p2"})), &inv())
                .unwrap_err()
                .is_conflict()
        );
        assert!(
            validate(&Record::from(json!({"parent": null})), &inv())
                .unwrap_err()
                .is_conflict()
        );
        validate(&Record::from(json!({"parent": "p1"})), &inv()).unwrap();
    }
}
