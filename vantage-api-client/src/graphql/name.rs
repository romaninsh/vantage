//! The GraphQL `Name` grammar, `/[_A-Za-z][_0-9A-Za-z]*/`.
//!
//! Every name the renderer splices into a document — selected fields,
//! object keys, the root field, argument names, order fields — passes
//! through here first. String *values* are escaped instead (see
//! `select::render`); a name has no escape syntax, so anything outside the
//! grammar could only change the meaning of the document.

use serde_json::Value;
use vantage_core::{Result, error};

/// Refuse `name` unless it is a single GraphQL `Name`. `kind` says what the
/// name is used as (`"root field"`, `"order field"`, …) for the error.
pub(crate) fn validate(kind: &'static str, name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid = chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric());
    if valid {
        Ok(())
    } else {
        Err(error!(
            "GraphQL name is not valid",
            kind = kind,
            name = name
        ))
    }
}

/// A dotted path (`run.commit.hash`), each segment a `Name`.
pub(crate) fn validate_path(kind: &'static str, path: &str) -> Result<()> {
    if path
        .split('.')
        .all(|segment| validate(kind, segment).is_ok())
    {
        Ok(())
    } else {
        Err(error!(
            "GraphQL name is not valid",
            kind = kind,
            name = path
        ))
    }
}

/// Every object key inside `value`, at any depth — what an inline argument
/// object renders as unquoted names.
pub(crate) fn validate_object_keys(kind: &'static str, value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                validate(kind, key)?;
                validate_object_keys(kind, inner)?;
            }
            Ok(())
        }
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| validate_object_keys(kind, item)),
        _ => Ok(()),
    }
}

/// Literal root arguments: each top-level key is a `"root argument"`, every
/// key nested in its value a `"root argument key"`.
pub(crate) fn validate_root_args(args: &Value) -> Result<()> {
    if let Value::Object(map) = args {
        for (name, value) in map {
            validate("root argument", name)?;
            validate_object_keys("root argument key", value)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_follow_the_grammar() {
        for ok in ["id", "_id", "__typename", "a1", "mission_name", "X"] {
            assert!(validate("field", ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "1a",
            "a-b",
            "a b",
            "a}",
            "a.b",
            "é",
            "a\n",
            "id { secret }",
        ] {
            let err = validate("field", bad).unwrap_err().to_string();
            assert!(err.contains("GraphQL name is not valid"), "{bad}: {err}");
        }
    }

    #[test]
    fn paths_are_checked_segment_by_segment() {
        assert!(validate_path("field", "run.commit.hash").is_ok());
        for bad in ["run.", ".run", "run..id", "run.1d", "run.id }"] {
            assert!(validate_path("field", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn object_keys_are_checked_at_every_depth() {
        assert!(validate_object_keys("key", &json!({ "a": { "_eq": [{ "b": 1 }] } })).is_ok());
        assert!(validate_object_keys("key", &json!({ "a": [{ "b c": 1 }] })).is_err());
        // Values are not names.
        assert!(validate_object_keys("key", &json!({ "a": "b c }" })).is_ok());
    }
}
