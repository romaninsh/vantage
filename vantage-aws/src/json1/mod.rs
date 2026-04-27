//! `AwsJson1` — TableSource for AWS JSON-1.1 services.
//!
//! Internal value type is `ciborium::Value`, matching `AnyTable`'s
//! carrier — so any `Table<AwsJson1, E>` wraps directly with
//! `AnyTable::new(table)` for `with_foreign` registration.
//!
//! The `Table::new` name encodes `"service/target"` (e.g.
//! `"logs/Logs_20140328.DescribeLogGroups"`); the `array_key` passed to
//! `aws.json1(...)` tells us where in the response the row array lives.
//! Conditions on the table fold into the JSON request body as
//! `{ field: value }`. v0 returns the first page only.
//!
//! Trait impls (`DataSource`, `ExprDataSource`, `TableSource`) live
//! under `impls/` to keep this file focused on the data shape and the
//! private helpers.

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

#[derive(Clone, Debug)]
pub struct AwsJson1 {
    account: AwsAccount,
    array_key: String,
}

impl AwsJson1 {
    pub(crate) fn new(account: AwsAccount, array_key: String) -> Self {
        Self { account, array_key }
    }

    /// Run the configured RPC, returning the parsed JSON response.
    /// Deferred conditions get materialised into `Eq` first via
    /// [`Self::resolve_conditions`].
    pub(crate) async fn execute_rpc(
        &self,
        table_name: &str,
        conditions: &[AwsCondition],
    ) -> Result<JsonValue> {
        let (service, target) = parse_endpoint(table_name)?;
        let resolved = self.resolve_conditions(conditions).await?;
        let body = build_body(&resolved)?;
        json1_call(&self.account, service, target, &JsonValue::Object(body)).await
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

    /// Pull the configured array out of a successful response and build
    /// records keyed by `id_field`. Each value is converted from
    /// `serde_json::Value` to `ciborium::Value` via the serde bridge.
    pub(crate) fn parse_records(
        &self,
        resp: JsonValue,
        id_field: Option<&str>,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let array = resp
            .get(&self.array_key)
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                error!(
                    "AWS response missing expected array key",
                    array_key = self.array_key.as_str(),
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
}

/// Parse `"service/target"` into `("service", "target")`. Anything else
/// is an error — the table name *is* the routing info for JSON-1.1.
fn parse_endpoint(table_name: &str) -> Result<(&str, &str)> {
    table_name.split_once('/').ok_or_else(|| {
        error!(
            "AwsJson1 table name must be \"service/target\" — got",
            name = table_name
        )
    })
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
    fn parse_endpoint_splits_on_slash() {
        let (service, target) = parse_endpoint("logs/Logs_20140328.DescribeLogGroups").unwrap();
        assert_eq!(service, "logs");
        assert_eq!(target, "Logs_20140328.DescribeLogGroups");
    }

    #[test]
    fn parse_endpoint_rejects_missing_slash() {
        let err = parse_endpoint("DescribeLogGroups").unwrap_err();
        assert!(format!("{err}").contains("service/target"));
    }
}
