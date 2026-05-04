//! Thin DynamoDB transport — Scan, GetItem, PutItem, DeleteItem.
//!
//! Wraps `crate::json1::json_aws_call` with the JSON-1.0 content type
//! and the `DynamoDB_20120810.*` target prefix. Higher-level concerns
//! (id extraction, condition rendering, pagination) live in
//! `impls/table_source.rs`.

use serde_json::{Map as JsonMap, Value as JsonValue, json};
use vantage_core::Result;

use crate::account::AwsAccount;
use crate::json1::json_aws_call;

const SERVICE: &str = "dynamodb";
const CONTENT_TYPE: &str = "application/x-amz-json-1.0";
const API_VERSION: &str = "DynamoDB_20120810";

async fn call(aws: &AwsAccount, action: &str, body: JsonValue) -> Result<JsonValue> {
    let target = format!("{API_VERSION}.{action}");
    json_aws_call(aws, SERVICE, &target, &body, CONTENT_TYPE).await
}

pub(crate) async fn scan(
    aws: &AwsAccount,
    table: &str,
    limit: Option<i64>,
    select_count: bool,
) -> Result<JsonValue> {
    let mut body = json!({ "TableName": table });
    if let Some(n) = limit {
        body["Limit"] = json!(n);
    }
    if select_count {
        body["Select"] = json!("COUNT");
    }
    call(aws, "Scan", body).await
}

pub(crate) async fn get_item(
    aws: &AwsAccount,
    table: &str,
    key: JsonMap<String, JsonValue>,
) -> Result<JsonValue> {
    let body = json!({ "TableName": table, "Key": key });
    call(aws, "GetItem", body).await
}

pub(crate) async fn put_item(
    aws: &AwsAccount,
    table: &str,
    item: JsonMap<String, JsonValue>,
) -> Result<JsonValue> {
    let body = json!({ "TableName": table, "Item": item });
    call(aws, "PutItem", body).await
}

pub(crate) async fn delete_item(
    aws: &AwsAccount,
    table: &str,
    key: JsonMap<String, JsonValue>,
) -> Result<JsonValue> {
    let body = json!({ "TableName": table, "Key": key });
    call(aws, "DeleteItem", body).await
}
