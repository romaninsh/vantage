//! JSON-1.1 protocol implementation for `AwsAccount`.
//!
//! `AwsAccount` itself is the `TableSource` — there's no separate
//! per-operation wrapper. All per-operation configuration lives in the
//! table name, parsed as:
//!
//! ```text
//! "array_key:service/target"
//!     │       │       └── X-Amz-Target header value (e.g. "Logs_20140328.DescribeLogGroups")
//!     │       └────────── service code, also the URL hostname segment ("logs", "ecs", …)
//!     └────────────────── response field that holds the row array ("logGroups", "events", …)
//! ```
//!
//! Conditions on the table fold into the JSON request body as
//! `{ field: value }`. v0 returns the first page only; deferred
//! conditions resolve via [`AwsAccount::resolve_conditions`] before
//! the body is built.
//!
//! Trait impls (`DataSource`, `ExprDataSource`, `TableSource`) live
//! under `impls/` to keep this file focused on protocol mechanics.

mod impls;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_types::Record;

use crate::account::AwsAccount;
use crate::condition::{AwsCondition, build_body};
use crate::transport::json1_call;

impl AwsAccount {
    /// Run the configured RPC, returning the parsed JSON response.
    /// Deferred conditions get materialised into `Eq` first via
    /// [`Self::resolve_conditions`].
    pub(crate) async fn execute_rpc(
        &self,
        table_name: &str,
        conditions: &[AwsCondition],
    ) -> Result<JsonValue> {
        let (_array_key, service, target) = parse_table_name(table_name)?;
        let resolved = self.resolve_conditions(conditions).await?;
        let body = build_body(&resolved)?;
        json1_call(self, service, target, &JsonValue::Object(body)).await
    }

    /// Pull the configured array out of a successful response and build
    /// records keyed by `id_field`. Each value is converted from
    /// `serde_json::Value` to `ciborium::Value` via the serde bridge.
    pub(crate) fn parse_records(
        &self,
        table_name: &str,
        resp: JsonValue,
        id_field: Option<&str>,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let (array_key, _service, _target) = parse_table_name(table_name)?;
        let array = resp
            .get(array_key)
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                error!(
                    "AWS response missing expected array key",
                    array_key = array_key,
                    body = format!("{}", resp)
                )
            })?
            .clone();

        let mut out = IndexMap::with_capacity(array.len());
        for (idx, item) in array.into_iter().enumerate() {
            let obj = match item {
                JsonValue::Object(map) => map,
                other => {
                    return Err(error!(
                        "AWS response array entry is not an object",
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

            let record: Record<CborValue> = obj
                .into_iter()
                .map(|(k, v)| (k, json_to_cbor(v)))
                .collect();
            out.insert(id, record);
        }
        Ok(out)
    }

    /// Walk conditions and materialise any `Deferred` into `Eq` by
    /// running the embedded expression. AWS doesn't accept multi-value
    /// filters, so the resolved value list must contain exactly one
    /// element; zero or more is a hard error.
    async fn resolve_conditions(
        &self,
        conditions: &[AwsCondition],
    ) -> Result<Vec<AwsCondition>> {
        let mut out = Vec::with_capacity(conditions.len());
        for cond in conditions {
            match cond {
                AwsCondition::Deferred { field, source } => {
                    let payload = ExprDataSource::execute(self, source).await?;
                    let values = match payload {
                        CborValue::Array(items) => items,
                        other => vec![other],
                    };
                    match values.len() {
                        1 => out.push(AwsCondition::Eq {
                            field: field.clone(),
                            value: values.into_iter().next().unwrap(),
                        }),
                        0 => {
                            return Err(error!(
                                "Deferred condition resolved to zero values — \
                                 source query returned nothing",
                                field = field.as_str()
                            ));
                        }
                        n => {
                            return Err(error!(
                                "AWS doesn't accept multi-value filters; \
                                 deferred condition resolved to many",
                                field = field.as_str(),
                                count = n
                            ));
                        }
                    }
                }
                other => out.push(other.clone()),
            }
        }
        Ok(out)
    }
}

/// Parse `"array_key:service/target"` into its three components.
pub(crate) fn parse_table_name(name: &str) -> Result<(&str, &str, &str)> {
    let (array_key, rest) = name.split_once(':').ok_or_else(|| {
        error!(
            "AwsAccount table name must be \"array_key:service/target\" — got",
            name = name
        )
    })?;
    let (service, target) = rest.split_once('/').ok_or_else(|| {
        error!(
            "AwsAccount table name must be \"array_key:service/target\" — got",
            name = name
        )
    })?;
    Ok((array_key, service, target))
}

/// JSON → CBOR via ciborium's serde bridge. JSON's value space is a
/// strict subset of CBOR's, so this is lossless and never fails for
/// well-formed `serde_json::Value`.
fn json_to_cbor(v: JsonValue) -> CborValue {
    CborValue::serialized(&v).expect("json → cbor cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_components() {
        let (array_key, service, target) =
            parse_table_name("logGroups:logs/Logs_20140328.DescribeLogGroups").unwrap();
        assert_eq!(array_key, "logGroups");
        assert_eq!(service, "logs");
        assert_eq!(target, "Logs_20140328.DescribeLogGroups");
    }

    #[test]
    fn rejects_missing_colon() {
        let err = parse_table_name("logs/Logs_20140328.DescribeLogGroups").unwrap_err();
        assert!(format!("{err}").contains("array_key:service/target"));
    }

    #[test]
    fn rejects_missing_slash() {
        let err = parse_table_name("logGroups:DescribeLogGroups").unwrap_err();
        assert!(format!("{err}").contains("array_key:service/target"));
    }
}
