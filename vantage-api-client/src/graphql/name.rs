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
