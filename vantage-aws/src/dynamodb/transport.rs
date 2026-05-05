//! Thin DynamoDB transport — Scan, GetItem, PutItem, DeleteItem.
//!
//! Wraps `crate::json1::json_aws_call` with the JSON-1.0 content type
//! and the `DynamoDB_20120810.*` target prefix. Higher-level concerns
//! (id extraction, condition rendering, pagination) live in
//! `impls/table_source.rs`.

use serde_json::{Map as JsonMap, Value as JsonValue, json};
use vantage_core::Result;

use crate::account::AwsAccount;
use crate::dynamodb::condition::ResolvedFilter;
use crate::dynamodb::wire::attr_to_json;
use crate::json1::json_aws_call;

const SERVICE: &str = "dynamodb";
const CONTENT_TYPE: &str = "application/x-amz-json-1.0";
const API_VERSION: &str = "DynamoDB_20120810";

async fn call(aws: &AwsAccount, action: &str, body: JsonValue) -> Result<JsonValue> {
    let target = format!("{API_VERSION}.{action}");
    json_aws_call(aws, SERVICE, &target, &body, CONTENT_TYPE).await
}

/// Run a DynamoDB Scan, walking `LastEvaluatedKey` until exhaustion
/// when the caller didn't pin a `Limit`. Without this, anything past
/// the 1MB-per-page boundary would silently drop — incomplete lists,
/// under-counts, partial deletes. When `limit` is `Some(n)` the caller
/// has explicitly asked for at most one page, so we don't paginate.
///
/// COUNT-mode pagination matters too: DynamoDB returns the per-page
/// count, not the table-wide total, and a heavily-filtered Scan can
/// reach `LastEvaluatedKey` with `Count: 0` after examining 1MB of raw
/// data — we keep walking until the cursor is gone.
///
/// The returned `JsonValue` is a synthetic merged shape: `Items` is the
/// concatenation of every page; `Count` is the sum.
pub(crate) async fn scan(
    aws: &AwsAccount,
    table: &str,
    limit: Option<i64>,
    select_count: bool,
    filter: Option<&ResolvedFilter>,
) -> Result<JsonValue> {
    if limit.is_some() {
        return scan_page(aws, table, limit, select_count, filter, None).await;
    }

    let mut all_items: Vec<JsonValue> = Vec::new();
    let mut total_count: i64 = 0;
    let mut start_key: Option<JsonValue> = None;

    loop {
        let resp = scan_page(aws, table, None, select_count, filter, start_key.as_ref()).await?;
        if select_count {
            if let Some(c) = resp.get("Count").and_then(|v| v.as_i64()) {
                total_count += c;
            }
        } else if let Some(arr) = resp.get("Items").and_then(|v| v.as_array()) {
            all_items.extend(arr.iter().cloned());
        }

        match resp.get("LastEvaluatedKey").cloned() {
            Some(k) if !k.is_null() => start_key = Some(k),
            _ => break,
        }
    }

    Ok(if select_count {
        json!({ "Count": total_count })
    } else {
        let n = all_items.len() as i64;
        json!({ "Items": all_items, "Count": n })
    })
}

async fn scan_page(
    aws: &AwsAccount,
    table: &str,
    limit: Option<i64>,
    select_count: bool,
    filter: Option<&ResolvedFilter>,
    start_key: Option<&JsonValue>,
) -> Result<JsonValue> {
    let mut body = json!({ "TableName": table });
    if let Some(n) = limit {
        body["Limit"] = json!(n);
    }
    if select_count {
        body["Select"] = json!("COUNT");
    }
    if let Some(f) = filter
        && !f.is_empty()
    {
        body["FilterExpression"] = json!(f.expression);
        if !f.names.is_empty() {
            let mut names = JsonMap::new();
            for (k, v) in &f.names {
                names.insert(k.clone(), JsonValue::String(v.clone()));
            }
            body["ExpressionAttributeNames"] = JsonValue::Object(names);
        }
        if !f.values.is_empty() {
            let mut values = JsonMap::new();
            for (k, v) in &f.values {
                values.insert(k.clone(), attr_to_json(v)?);
            }
            body["ExpressionAttributeValues"] = JsonValue::Object(values);
        }
    }
    if let Some(k) = start_key {
        body["ExclusiveStartKey"] = k.clone();
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
