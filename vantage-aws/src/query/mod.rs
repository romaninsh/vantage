//! AWS Query protocol — the older form-encoded / XML wire used by IAM,
//! STS, EC2, ELBv1, SES, etc.
//!
//! Two entry points:
//!   - [`execute`] form-encodes the request, signs, sends, and returns
//!     the XML response normalised into `serde_json::Value` for shared
//!     downstream handling.
//!   - [`parse_records`] plucks the configured array out of the
//!     normalised response and folds it into `Record<CborValue>`s.
//!
//! Target syntax for Query operations is `"VERSION.Action"` — e.g.
//! `"2010-05-08.ListUsers"` for IAM. The version is service-wide and
//! constant; we keep it inside the table name for grammatical
//! consistency with the JSON-1.1 `"prefix.Operation"` `X-Amz-Target`.

mod transport;
mod xml;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::account::AwsAccount;
use crate::condition::{AwsCondition, build_query_form};
use crate::dispatch::{OperationDescriptor, json_to_cbor};

pub(crate) use transport::query_call;

/// Build the form body, post it, and normalise the XML response into
/// `JsonValue`. The Action and Version pulled out of `op.target` are
/// added to the form body alongside the resolved conditions.
pub(crate) async fn execute(
    account: &AwsAccount,
    op: &OperationDescriptor<'_>,
    resolved: &[AwsCondition],
) -> Result<JsonValue> {
    let (version, action) = split_target(op.target)?;

    let mut form = vec![
        ("Action".to_string(), action.to_string()),
        ("Version".to_string(), version.to_string()),
    ];
    form.extend(build_query_form(resolved)?);

    let xml = query_call(account, op.service, &form).await?;
    xml::parse_query_response(&xml)
}

/// Pull `op.array_key` out of the normalised response. Query payloads
/// wrap lists as `<Foo><member>...</member></Foo>`, which the XML
/// normaliser hoists into a JSON array — so the happy-path lookup is
/// the same single-key access JSON-1.1 does. Self-closing list
/// elements (`<Foo/>`, returned when AWS has nothing to list) collapse
/// to the empty string in the JSON shape; we treat that as an empty
/// array, since "no rows" is a valid IAM/etc. response, not an error.
pub(crate) fn parse_records(
    op: &OperationDescriptor<'_>,
    resp: JsonValue,
    id_field: Option<&str>,
) -> Result<IndexMap<String, Record<CborValue>>> {
    let array = match resp.get(op.array_key) {
        Some(JsonValue::Array(a)) => a.clone(),
        Some(JsonValue::String(s)) if s.is_empty() => Vec::new(),
        Some(other) => {
            return Err(error!(
                "AWS Query response array key has unexpected shape — \
                 expected a list of <member> elements",
                array_key = op.array_key,
                got = format!("{:?}", other),
                body = format!("{}", resp)
            ));
        }
        None => {
            return Err(error!(
                "AWS Query response missing expected array key",
                array_key = op.array_key,
                body = format!("{}", resp)
            ));
        }
    };

    let scalar_field = id_field.unwrap_or("value");

    let mut out = IndexMap::with_capacity(array.len());
    for (idx, item) in array.into_iter().enumerate() {
        let obj = match item {
            JsonValue::Object(map) => map,
            JsonValue::String(_) | JsonValue::Number(_) => {
                let mut m = serde_json::Map::new();
                m.insert(scalar_field.to_string(), item);
                m
            }
            other => {
                return Err(error!(
                    "AWS Query response array entry is not an object or scalar",
                    index = idx,
                    got = format!("{:?}", other)
                ));
            }
        };

        let id = id_field
            .and_then(|f| obj.get(f))
            .and_then(|v| match v {
                JsonValue::String(s) => Some(s.clone()),
                JsonValue::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| idx.to_string());

        let record: Record<CborValue> =
            obj.into_iter().map(|(k, v)| (k, json_to_cbor(v))).collect();
        out.insert(id, record);
    }
    Ok(out)
}

/// Split the `target` field into `(version, action)`.
/// Format: `"YYYY-MM-DD.ActionName"` — the version is whatever AWS
/// publishes in the API reference for the service.
fn split_target(target: &str) -> Result<(&str, &str)> {
    target.split_once('.').ok_or_else(|| {
        error!(
            "Query target must be \"VERSION.Action\" (e.g. \"2010-05-08.ListUsers\") — got",
            target = target
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_splits_into_version_and_action() {
        let (v, a) = split_target("2010-05-08.ListUsers").unwrap();
        assert_eq!(v, "2010-05-08");
        assert_eq!(a, "ListUsers");
    }

    #[test]
    fn target_without_dot_errors() {
        assert!(split_target("ListUsers").is_err());
    }
}
