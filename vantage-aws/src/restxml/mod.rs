//! REST-XML protocol — the wire used by S3 (and a handful of older
//! services like Route 53 and CloudFront).
//!
//! Two entry points:
//!   - [`execute`] builds the URL from a `METHOD path-template` target,
//!     fills `{Placeholder}` segments from conditions, and posts the
//!     remainder to the query string.
//!   - [`parse_records`] strips the outer XML root and plucks the
//!     configured array — supports dotted lookup (`Buckets.Bucket`)
//!     since S3 nests its lists.
//!
//! Target syntax: `"METHOD path?static-query"` — e.g.
//! `"GET /{Bucket}?list-type=2"` for `ListObjectsV2`. Conditions whose
//! field name matches a `{Placeholder}` get spliced into the path; all
//! others append to the query string.
//!
//! S3 mandates `x-amz-content-sha256` on every signed request, so the
//! transport adds it unconditionally. Other REST-XML services accept
//! it without complaint, so we don't gate on the service name.

pub(crate) mod transport;
mod xml;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_types::Record;

use crate::account::AwsAccount;
use crate::condition::AwsCondition;
use crate::dispatch::{OperationDescriptor, json_to_cbor, lookup_path};

pub(crate) use transport::restxml_call;

/// Build the URL from `METHOD path?query`, fill path placeholders from
/// conditions, send the (empty-body) request, and parse the XML
/// response into a normalised `JsonValue`.
pub(crate) async fn execute(
    account: &AwsAccount,
    op: &OperationDescriptor<'_>,
    resolved: &[AwsCondition],
) -> Result<JsonValue> {
    let (method, path, query) = transport::build_request(op.target, resolved)?;
    let body = restxml_call(account, op.service, &method, &path, &query).await?;
    xml::parse_xml_response(&body)
}

/// Pull `op.array_key` out of the parsed response. S3 wraps its lists
/// inside a named element (`<Buckets>` containing repeated `<Bucket>`),
/// not the IAM-style `<member>` shim, so callers point at the array
/// with a dotted path: `array_key = "Buckets.Bucket"`.
///
/// Plain (no dot) array keys still work for flat shapes like S3's
/// `<Contents>` repeats inside `<ListBucketResult>`.
pub(crate) fn parse_records(
    op: &OperationDescriptor<'_>,
    resp: JsonValue,
    id_field: Option<&str>,
) -> Result<IndexMap<String, Record<CborValue>>> {
    let array = match lookup_path(&resp, op.array_key) {
        Some(JsonValue::Array(a)) => a.clone(),
        // Single repeating element: the XML normaliser kept it scalar.
        // Promote to a one-element array so the caller sees a row.
        Some(JsonValue::Object(_)) => vec![lookup_path(&resp, op.array_key).unwrap().clone()],
        // Empty self-closing element collapses to "" — treat as no rows.
        Some(JsonValue::String(s)) if s.is_empty() => Vec::new(),
        Some(other) => {
            return Err(error!(
                "AWS REST-XML response array key has unexpected shape",
                array_key = op.array_key,
                got = format!("{:?}", other),
                body = format!("{}", resp)
            ));
        }
        None => Vec::new(),
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
                    "AWS REST-XML response array entry is not an object or scalar",
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
