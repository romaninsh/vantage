//! Decoding a `DocumentValidationFailure` (code 121) into the rules the
//! document broke, e.g. `min_level: maximum 60`.

use bson::{Bson, Document};
use mongodb::error::{ErrorKind, WriteFailure};
use vantage_core::{VantageError, error};

const DOCUMENT_VALIDATION_FAILURE: i32 = 121;

/// An invalid-value error naming each broken rule, for a write the
/// collection's validator refused; `None` for any other error.
pub(crate) fn validation_failed(
    e: &mongodb::error::Error,
    collection: &str,
) -> Option<VantageError> {
    let ErrorKind::Write(WriteFailure::WriteError(we)) = &*e.kind else {
        return None;
    };
    if we.code != DOCUMENT_VALIDATION_FAILURE {
        return None;
    }
    let mut rules = Vec::new();
    if let Some(Bson::Document(root)) = we.details.as_ref().and_then(|d| d.get("details")) {
        collect(root, "", &mut rules);
    }
    let message = if rules.is_empty() {
        "Document failed validation".to_string()
    } else {
        rules.join("; ")
    };
    Some(error!(message, collection = collection, details = e.to_string()).mark_invalid_value())
}

/// Walk one node of the server's `errInfo.details` tree, adding a
/// `path: rule` line per unsatisfied leaf rule.
fn collect(node: &Document, path: &str, out: &mut Vec<String>) {
    let path = match node.get("itemIndex").and_then(as_index) {
        Some(i) => format!("{path}[{i}]"),
        None => path.to_string(),
    };
    let before = out.len();

    if let Ok(props) = node.get_array("propertiesNotSatisfied") {
        for prop in props.iter().filter_map(Bson::as_document) {
            let name = prop.get_str("propertyName").unwrap_or("?");
            children(prop.get("details"), &join(&path, name), out);
        }
    }
    for key in ["schemaRulesNotSatisfied", "clausesNotSatisfied", "details"] {
        children(node.get(key), &path, out);
    }

    if out.len() == before {
        leaf(node, &path, out);
    }
}

/// Collect from a child node or a list of them.
fn children(value: Option<&Bson>, path: &str, out: &mut Vec<String>) {
    match value {
        Some(Bson::Array(items)) => {
            for child in items.iter().filter_map(Bson::as_document) {
                collect(child, path, out);
            }
        }
        Some(Bson::Document(child)) => collect(child, path, out),
        _ => {}
    }
}

fn leaf(node: &Document, path: &str, out: &mut Vec<String>) {
    let Ok(op) = node.get_str("operatorName") else {
        return;
    };
    if let Ok(missing) = node.get_array("missingProperties") {
        for name in missing.iter().filter_map(Bson::as_str) {
            out.push(format!("{}: required", join(path, name)));
        }
        return;
    }
    if let Ok(extra) = node.get_array("additionalProperties") {
        for name in extra.iter().filter_map(Bson::as_str) {
            out.push(format!("{}: not allowed", join(path, name)));
        }
        return;
    }
    let specified = node.get_document("specifiedAs").ok();
    let rule = match specified.and_then(|s| s.get(op)) {
        Some(value) => format!("{op} {}", show(value)),
        // A query-operator validator: `specifiedAs: { field: { $op: value } }`.
        None => match specified.and_then(|s| s.iter().next()) {
            Some((field, Bson::Document(expr))) if path.is_empty() => {
                let rule = expr
                    .iter()
                    .next()
                    .map(|(o, v)| format!("{} {}", o.trim_start_matches('$'), show(v)))
                    .unwrap_or_else(|| op.trim_start_matches('$').to_string());
                out.push(format!("{field}: {rule}"));
                return;
            }
            _ => op.trim_start_matches('$').to_string(),
        },
    };
    if path.is_empty() {
        out.push(rule);
    } else {
        out.push(format!("{path}: {rule}"));
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

fn as_index(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(i) => Some(i64::from(*i)),
        Bson::Int64(i) => Some(*i),
        _ => None,
    }
}

/// A rule's value as a person reads it: text unquoted, lists comma-joined.
fn show(value: &Bson) -> String {
    match value {
        Bson::String(s) => s.clone(),
        Bson::Array(items) => items.iter().map(show).collect::<Vec<_>>().join(", "),
        Bson::Double(f) if f.fract() == 0.0 => format!("{f:.0}"),
        other => other.to_string(),
    }
}
