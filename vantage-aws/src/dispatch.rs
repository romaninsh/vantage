//! Protocol dispatcher for `AwsAccount`.
//!
//! AWS speaks several wire protocols (JSON-1.1, Query, REST-JSON, …)
//! and the choice is encoded in the table name's prefix:
//!
//! ```text
//! "{protocol}/{array_key}:{service}/{target}"
//!
//! json1/logGroups:logs/Logs_20140328.DescribeLogGroups
//! query/Users:iam/2010-05-08.ListUsers
//! ```
//!
//! Each protocol owns its own request build / send / parse code under
//! its own module (`json1::`, `query::`). This file only knows the
//! grammar and the two-line match that picks the right module.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_types::Record;

use crate::account::AwsAccount;
use crate::condition::AwsCondition;
use crate::{json1, query};

/// Which AWS wire protocol an operation uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    /// `application/x-amz-json-1.1`. CloudWatch Logs, ECS, KMS, DynamoDB, …
    Json1,
    /// AWS Query (form-encoded request, XML response). IAM, STS, EC2, …
    Query,
}

/// Parsed table name. Borrows from the input string.
#[derive(Debug)]
pub(crate) struct OperationDescriptor<'a> {
    pub protocol: Protocol,
    pub array_key: &'a str,
    pub service: &'a str,
    pub target: &'a str,
}

impl AwsAccount {
    /// Run the configured RPC, returning a normalised JSON response.
    /// Both protocols converge on `serde_json::Value` here so
    /// `parse_records` can stay protocol-agnostic at the array-pluck
    /// step. Deferred conditions get materialised into `Eq` first.
    pub(crate) async fn execute_rpc(
        &self,
        table_name: &str,
        conditions: &[AwsCondition],
    ) -> Result<JsonValue> {
        let op = parse_table_name(table_name)?;
        let resolved = self.resolve_conditions(conditions).await?;
        match op.protocol {
            Protocol::Json1 => json1::execute(self, &op, &resolved).await,
            Protocol::Query => query::execute(self, &op, &resolved).await,
        }
    }

    /// Pull records out of a successful response. Each protocol owns
    /// the array-extraction since their wire shapes differ (json1
    /// returns the array at the top level; query wraps it in
    /// `{Action}Result`).
    pub(crate) fn parse_records(
        &self,
        table_name: &str,
        resp: JsonValue,
        id_field: Option<&str>,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let op = parse_table_name(table_name)?;
        match op.protocol {
            Protocol::Json1 => json1::parse_records(&op, resp, id_field),
            Protocol::Query => query::parse_records(&op, resp, id_field),
        }
    }

    /// Walk conditions and materialise any `Deferred` into `Eq` by
    /// running the embedded expression. AWS doesn't accept multi-value
    /// filters, so the resolved value list must contain exactly one
    /// element; zero or more is a hard error.
    async fn resolve_conditions(&self, conditions: &[AwsCondition]) -> Result<Vec<AwsCondition>> {
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

/// Parse `"{protocol}/{array_key}:{service}/{target}"`.
pub(crate) fn parse_table_name(name: &str) -> Result<OperationDescriptor<'_>> {
    let bad = || {
        error!(
            "AwsAccount table name must be \"{protocol}/{array_key}:{service}/{target}\" — got",
            name = name
        )
    };

    let (proto_str, rest) = name.split_once('/').ok_or_else(bad)?;
    let protocol = match proto_str {
        "json1" => Protocol::Json1,
        "query" => Protocol::Query,
        other => {
            return Err(error!(
                "Unknown AWS protocol prefix — expected \"json1\" or \"query\"",
                got = other
            ));
        }
    };
    let (array_key, rest) = rest.split_once(':').ok_or_else(bad)?;
    let (service, target) = rest.split_once('/').ok_or_else(bad)?;

    if array_key.is_empty() || service.is_empty() || target.is_empty() {
        return Err(bad());
    }

    Ok(OperationDescriptor {
        protocol,
        array_key,
        service,
        target,
    })
}

/// JSON → CBOR via ciborium's serde bridge. JSON's value space is a
/// strict subset of CBOR's, so this is lossless and never fails for
/// well-formed `serde_json::Value`.
pub(crate) fn json_to_cbor(v: JsonValue) -> CborValue {
    CborValue::serialized(&v).expect("json → cbor cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json1_form() {
        let op = parse_table_name("json1/logGroups:logs/Logs_20140328.DescribeLogGroups").unwrap();
        assert_eq!(op.protocol, Protocol::Json1);
        assert_eq!(op.array_key, "logGroups");
        assert_eq!(op.service, "logs");
        assert_eq!(op.target, "Logs_20140328.DescribeLogGroups");
    }

    #[test]
    fn parses_query_form() {
        let op = parse_table_name("query/Users:iam/2010-05-08.ListUsers").unwrap();
        assert_eq!(op.protocol, Protocol::Query);
        assert_eq!(op.array_key, "Users");
        assert_eq!(op.service, "iam");
        assert_eq!(op.target, "2010-05-08.ListUsers");
    }

    #[test]
    fn rejects_unknown_protocol() {
        let err = parse_table_name("xml/Users:iam/2010-05-08.ListUsers").unwrap_err();
        assert!(format!("{err}").contains("Unknown AWS protocol prefix"));
    }

    #[test]
    fn rejects_missing_protocol_prefix() {
        // No leading `proto/` segment — single slash splits into ("logGroups:logs", "...")
        // and the colon split also fails. Surface the grammar message either way.
        let err = parse_table_name("logGroups:logs/Logs_20140328.DescribeLogGroups").unwrap_err();
        assert!(
            format!("{err}").contains("Unknown AWS protocol prefix")
                || format!("{err}").contains("must be \"")
        );
    }

    #[test]
    fn rejects_missing_colon() {
        let err = parse_table_name("json1/logs/Logs_20140328.DescribeLogGroups").unwrap_err();
        assert!(format!("{err}").contains("must be \""));
    }

    #[test]
    fn rejects_missing_target_slash() {
        let err = parse_table_name("json1/logGroups:DescribeLogGroups").unwrap_err();
        assert!(format!("{err}").contains("must be \""));
    }

    #[test]
    fn rejects_empty_components() {
        let err = parse_table_name("json1/:logs/Logs.X").unwrap_err();
        assert!(format!("{err}").contains("must be \""));
    }
}
