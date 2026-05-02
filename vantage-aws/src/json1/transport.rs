//! AWS JSON-1.1 RPC dispatcher.
//!
//! Single entry point used by `AwsJson1::list_table_values`. Builds the
//! signed POST, parses the JSON response, surfaces AWS error bodies
//! verbatim so callers can match on `__type` strings if they want.

use std::time::SystemTime;

use serde_json::Value;
use vantage_core::Result;
use vantage_core::error;

use crate::account::AwsAccount;
use crate::sign::sign_v4;

/// Issue a JSON-1.1 RPC and return the parsed response body.
///
/// Thin wrapper around [`json_aws_call`] for the 1.1 case — the only
/// thing it sets is the content type.
pub(crate) async fn json1_call(
    account: &AwsAccount,
    service: &str,
    target: &str,
    body: &Value,
) -> Result<Value> {
    json_aws_call(account, service, target, body, "application/x-amz-json-1.1").await
}

/// Underlying JSON-RPC dispatcher. Same shape as the 1.1 form but with
/// a caller-supplied content type — DynamoDB and a few siblings still
/// expect 1.0 (`application/x-amz-json-1.0`) on the wire.
///
/// `service` is the lowercased service code (e.g. `"logs"`, `"dynamodb"`),
/// which is both the SigV4 service name and the URL hostname segment.
/// `target` is the full `X-Amz-Target` header value.
pub(crate) async fn json_aws_call(
    account: &AwsAccount,
    service: &str,
    target: &str,
    body: &Value,
    content_type: &str,
) -> Result<Value> {
    let region = account.region();
    if region.is_empty() {
        return Err(error!(
            "AWS region is not configured — pass it to AwsAccount::new \
             or set AWS_REGION before calling AwsAccount::from_env"
        ));
    }
    let host = format!("{service}.{region}.amazonaws.com");
    let url = format!("https://{host}/");

    let body_bytes = serde_json::to_vec(body)
        .map_err(|e| error!("Failed to serialise JSON-1.1 request body", detail = e))?;

    let signing_headers = [
        ("host".to_string(), host.clone()),
        ("content-type".to_string(), content_type.to_string()),
        ("x-amz-target".to_string(), target.to_string()),
    ];

    let signed = sign_v4(
        account.access_key(),
        account.secret_key(),
        account.session_token(),
        region,
        service,
        "POST",
        &url,
        &signing_headers,
        &body_bytes,
        SystemTime::now(),
    )?;

    let mut req = account
        .http()
        .post(&url)
        .header("content-type", content_type)
        .header("x-amz-target", target)
        .body(body_bytes);
    for h in &signed {
        req = req.header(h.name.as_str(), h.value.as_str());
    }

    let resp = req.send().await.map_err(|e| {
        error!(
            "AWS JSON-RPC request failed",
            url = url.as_str(),
            target = target,
            detail = e
        )
    })?;

    let status = resp.status();
    let response_text = resp
        .text()
        .await
        .map_err(|e| error!("Failed to read AWS response body", detail = e))?;

    if !status.is_success() {
        return Err(error!(
            "AWS request returned error status",
            target = target,
            status = status.as_u16(),
            body = response_text
        ));
    }

    serde_json::from_str(&response_text).map_err(|e| {
        error!(
            "Failed to parse AWS JSON-RPC response",
            target = target,
            detail = e,
            body_preview = response_text.chars().take(200).collect::<String>()
        )
    })
}
