//! JSON-1.0 protocol — DynamoDB control + data plane.
//!
//! Wire shape is identical to JSON-1.1 (POST a JSON object,
//! `X-Amz-Target` selects the operation, response is a JSON object).
//! The only difference DynamoDB cares about is the content type,
//! `application/x-amz-json-1.0`, which is what trips up callers who try
//! to reuse [`crate::json1`] verbatim.
//!
//! Record extraction is delegated to [`crate::json1::parse_records`] —
//! the response shape is the same, including the dotted-path lookup
//! for nested arrays (DynamoDB's `Table` is a single object rather
//! than an array, which is fine: callers can target a different
//! `array_key` per operation).

use serde_json::Value as JsonValue;
use vantage_core::Result;

use crate::account::AwsAccount;
use crate::condition::{AwsCondition, build_json1_body};
use crate::dispatch::OperationDescriptor;
use crate::json1::json_aws_call;

pub(crate) async fn execute(
    account: &AwsAccount,
    op: &OperationDescriptor<'_>,
    resolved: &[AwsCondition],
) -> Result<JsonValue> {
    let body = build_json1_body(resolved)?;
    json_aws_call(
        account,
        op.service,
        op.target,
        &JsonValue::Object(body),
        "application/x-amz-json-1.0",
    )
    .await
}
