//! JSON-1.1 protocol — the wire used by CloudWatch, ECS, KMS,
//! DynamoDB control plane, etc.
//!
//! Two entry points:
//!   - [`execute`] builds the JSON request body, signs, sends.
//!   - [`parse_records`] plucks the configured array from the
//!     parsed response and folds it into `Record<CborValue>`s.
//!
//! Both are called from [`crate::dispatch`] after the protocol switch
//! has fired. The shape that distinguishes JSON-1.1 from sibling
//! protocols: request body is a JSON object posted with
//! `Content-Type: application/x-amz-json-1.1` and an `X-Amz-Target`
//! header naming the operation; response body is a JSON object whose
//! top-level key is the array of rows.

mod transport;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::account::AwsAccount;
use crate::condition::{AwsCondition, build_json1_body};
use crate::dispatch::{OperationDescriptor, json_to_cbor, lookup_path};

pub(crate) use transport::{json_aws_call, json1_call};

/// Build the JSON-1.1 request body and post it. `target` is used
/// verbatim as the `X-Amz-Target` header; `service` is both the SigV4
/// service name and the URL hostname segment.
pub(crate) async fn execute(
    account: &AwsAccount,
    op: &OperationDescriptor<'_>,
    resolved: &[AwsCondition],
) -> Result<JsonValue> {
    let body = build_json1_body(resolved)?;
    json1_call(account, op.service, op.target, &JsonValue::Object(body)).await
}

/// Pull the configured array out of the response and build records
/// keyed by `id_field`. Each value is converted from
/// `serde_json::Value` to `ciborium::Value` via the serde bridge.
///
/// Scalar array elements (strings/numbers) get wrapped as
/// `{<id_field>: value}` — this is what the ECS List* APIs return
/// (`clusterArns: ["arn:…", …]` instead of objects).
pub(crate) fn parse_records(
    op: &OperationDescriptor<'_>,
    resp: JsonValue,
    id_field: Option<&str>,
) -> Result<IndexMap<String, Record<CborValue>>> {
    let array = lookup_path(&resp, op.array_key)
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            error!(
                "AWS JSON-1.1 response missing expected array key",
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
                    "AWS response array entry is not an object or scalar",
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
