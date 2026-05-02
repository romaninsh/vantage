//! REST-JSON protocol — Lambda, API Gateway v2, and a few others.
//!
//! Same request shape as [`crate::restxml`] (METHOD + path-template,
//! placeholders filled from conditions, leftovers go to the query
//! string), but the response body is JSON. Records get plucked using
//! the dotted-path lookup so callers can target nested arrays
//! (`Functions`, `Aliases`, `Versions`, …).

mod transport;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::account::AwsAccount;
use crate::condition::AwsCondition;
use crate::dispatch::{OperationDescriptor, json_to_cbor, lookup_path};

pub(crate) async fn execute(
    account: &AwsAccount,
    op: &OperationDescriptor<'_>,
    resolved: &[AwsCondition],
) -> Result<JsonValue> {
    // REST-JSON request shape is identical to REST-XML, so reuse the
    // builder. Path templating, placeholder substitution, and query
    // assembly are all in one place.
    let (method, path, query) = crate::restxml::transport::build_request(op.target, resolved)?;
    transport::restjson_call(account, op.service, &method, &path, &query).await
}

pub(crate) fn parse_records(
    op: &OperationDescriptor<'_>,
    resp: JsonValue,
    id_field: Option<&str>,
) -> Result<IndexMap<String, Record<CborValue>>> {
    let array = lookup_path(&resp, op.array_key)
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            error!(
                "AWS REST-JSON response missing expected array key",
                array_key = op.array_key,
                body = format!("{}", resp)
            )
        })?
        .clone();

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
                    "AWS REST-JSON response array entry is not an object or scalar",
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
