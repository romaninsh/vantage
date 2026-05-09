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
use crate::{json1, json10, query, restjson, restxml};

/// Which AWS wire protocol an operation uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Protocol {
    /// `application/x-amz-json-1.1`. CloudWatch Logs, ECS, KMS, …
    Json1,
    /// `application/x-amz-json-1.0`. DynamoDB control + data plane.
    Json10,
    /// AWS Query (form-encoded request, XML response). IAM, STS, EC2, …
    Query,
    /// REST-XML over GET/POST. S3.
    RestXml,
    /// REST-JSON over GET/POST. Lambda, API Gateway v2, …
    RestJson,
}

/// Parsed table name. Borrows from the input string.
#[derive(Debug)]
pub(crate) struct OperationDescriptor<'a> {
    pub protocol: Protocol,
    pub array_key: &'a str,
    pub service: &'a str,
    pub target: &'a str,
    /// Continuation-token field name. Same string is used for both the
    /// request body field and the response body field — every CloudWatch
    /// Logs and ECS list API uses `nextToken` symmetrically. Encoded in
    /// the table name as `@<name>` after the array key:
    ///
    /// ```text
    /// json1/logStreams@nextToken:logs/Logs_20140328.DescribeLogStreams
    /// ```
    ///
    /// `None` means single-page (current behaviour). Other AWS protocols
    /// (Query, RestXml, RestJson) and asymmetric cursors (KMS's
    /// `Marker`/`NextMarker`, `GetLogEvents`'s forward/backward tokens)
    /// don't use this field — they'll need a richer descriptor when
    /// auto-pagination expands beyond json1.
    pub cursor: Option<&'a str>,
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

        // Auto-paginate when the descriptor carries a cursor name.
        // Scoped to JSON-1.1 / 1.0 because that's where every paginated
        // op uses the same field name in both directions and where the
        // response array sits at a known top-level key. Other protocols
        // need their own walk implementations (different cursor naming,
        // `IsTruncated` flags, XML wrapping) — they stay single-page.
        if let Some(cursor) = op.cursor
            && matches!(op.protocol, Protocol::Json1 | Protocol::Json10)
        {
            return self.walk_json_pages(&op, &resolved, cursor).await;
        }

        match op.protocol {
            Protocol::Json1 => json1::execute(self, &op, &resolved).await,
            Protocol::Json10 => json10::execute(self, &op, &resolved).await,
            Protocol::Query => query::execute(self, &op, &resolved).await,
            Protocol::RestXml => restxml::execute(self, &op, &resolved).await,
            Protocol::RestJson => restjson::execute(self, &op, &resolved).await,
        }
    }

    /// Walk a JSON-1.x paginated list operation by re-issuing the same
    /// request with the response's continuation token folded into the
    /// next request's body, until the token is gone (or
    /// [`AwsAccount::max_pages`] is hit). Items from each page are
    /// concatenated under the descriptor's `array_key`; non-array
    /// top-level fields are taken from the last page (none of the
    /// supported ops carry meaningful per-page metadata, so this is
    /// fine for now — see top-of-file note).
    async fn walk_json_pages(
        &self,
        op: &OperationDescriptor<'_>,
        resolved: &[AwsCondition],
        cursor_field: &str,
    ) -> Result<JsonValue> {
        let max_pages = self.max_pages();
        let mut conds: Vec<AwsCondition> = resolved.to_vec();

        let mut accumulated: Vec<JsonValue> = Vec::new();
        let mut pages: usize = 0;

        let mut merged = loop {
            let resp = match op.protocol {
                Protocol::Json1 => json1::execute(self, op, &conds).await?,
                Protocol::Json10 => json10::execute(self, op, &conds).await?,
                _ => unreachable!("walk_json_pages is gated on Json1/Json10"),
            };
            pages += 1;

            if let Some(arr) = lookup_path(&resp, op.array_key).and_then(|v| v.as_array()) {
                accumulated.extend(arr.iter().cloned());
            }

            let next_cursor = resp
                .get(cursor_field)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string());

            match next_cursor {
                Some(token) if max_pages.is_none_or(|cap| pages < cap) => {
                    // Replace any prior cursor condition before re-issuing.
                    conds.retain(|c| c.field() != cursor_field);
                    conds.push(AwsCondition::eq(cursor_field.to_string(), token));
                }
                _ => break resp,
            }
        };

        // Replace the array under `array_key` with the concatenated
        // results. `array_key` is always a single segment for the json1
        // ops we walk (no dotted paths in the supported descriptors).
        if let Some(obj) = merged.as_object_mut() {
            obj.insert(op.array_key.to_string(), JsonValue::Array(accumulated));
        }
        Ok(merged)
    }

    /// Pull records out of a successful response. Each protocol owns
    /// the array-extraction since their wire shapes differ (json1
    /// returns the array at the top level; query wraps it in
    /// `{Action}Result`; restxml/restjson follow REST conventions).
    pub(crate) fn parse_records(
        &self,
        table_name: &str,
        resp: JsonValue,
        id_field: Option<&str>,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let op = parse_table_name(table_name)?;
        match op.protocol {
            Protocol::Json1 | Protocol::Json10 => json1::parse_records(&op, resp, id_field),
            Protocol::Query => query::parse_records(&op, resp, id_field),
            Protocol::RestXml => restxml::parse_records(&op, resp, id_field),
            Protocol::RestJson => restjson::parse_records(&op, resp, id_field),
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
        "json10" => Protocol::Json10,
        "query" => Protocol::Query,
        "restxml" => Protocol::RestXml,
        "restjson" => Protocol::RestJson,
        other => {
            return Err(error!(
                "Unknown AWS protocol prefix — expected one of \
                 json1, json10, query, restxml, restjson",
                got = other
            ));
        }
    };
    let (array_key_raw, rest) = rest.split_once(':').ok_or_else(bad)?;
    let (service, target) = rest.split_once('/').ok_or_else(bad)?;

    // Optional `@cursor` suffix on the array key opts the operation
    // into auto-pagination — see [`OperationDescriptor::cursor`].
    let (array_key, cursor) = match array_key_raw.split_once('@') {
        Some((key, cursor)) if !cursor.is_empty() => (key, Some(cursor)),
        Some(_) => return Err(bad()),
        None => (array_key_raw, None),
    };

    if array_key.is_empty() || service.is_empty() || target.is_empty() {
        return Err(bad());
    }

    Ok(OperationDescriptor {
        protocol,
        array_key,
        service,
        target,
        cursor,
    })
}

/// JSON → CBOR via ciborium's serde bridge. JSON's value space is a
/// strict subset of CBOR's, so this is lossless and never fails for
/// well-formed `serde_json::Value`.
pub(crate) fn json_to_cbor(v: JsonValue) -> CborValue {
    CborValue::serialized(&v).expect("json → cbor cannot fail")
}

/// Walk a dotted path (`"Buckets.Bucket"`) through a JSON value, taking
/// each segment as an object key. Returns `None` if any segment misses.
/// Plain (non-dotted) keys are passed through `Value::get` directly.
pub(crate) fn lookup_path<'a>(value: &'a JsonValue, path: &str) -> Option<&'a JsonValue> {
    let mut cur = value;
    for part in path.split('.') {
        cur = cur.get(part)?;
    }
    Some(cur)
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
        assert_eq!(op.cursor, None);
    }

    #[test]
    fn parses_query_form() {
        let op = parse_table_name("query/Users:iam/2010-05-08.ListUsers").unwrap();
        assert_eq!(op.protocol, Protocol::Query);
        assert_eq!(op.array_key, "Users");
        assert_eq!(op.service, "iam");
        assert_eq!(op.target, "2010-05-08.ListUsers");
        assert_eq!(op.cursor, None);
    }

    #[test]
    fn parses_cursor_suffix() {
        let op = parse_table_name(
            "json1/logStreams@nextToken:logs/Logs_20140328.DescribeLogStreams",
        )
        .unwrap();
        assert_eq!(op.array_key, "logStreams");
        assert_eq!(op.cursor, Some("nextToken"));
    }

    #[test]
    fn rejects_empty_cursor_suffix() {
        let err = parse_table_name("json1/logStreams@:logs/Logs_20140328.DescribeLogStreams")
            .unwrap_err();
        assert!(format!("{err}").contains("must be \""));
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
