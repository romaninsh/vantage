//! REST-JSON HTTP transport (Lambda etc.).
//!
//! Mirror of [`crate::restxml::transport`] but for JSON responses and
//! without the `x-amz-content-sha256` header — Lambda accepts the
//! standard SigV4 minimum-signed-headers set without it. Empty body
//! since v0 is read-only.

use std::time::SystemTime;

use serde_json::Value as JsonValue;
use vantage_core::{Result, error};

use crate::account::AwsAccount;
use crate::sign::sign_v4;

pub(crate) async fn restjson_call(
    account: &AwsAccount,
    service: &str,
    method: &str,
    path: &str,
    query: &[(String, String)],
) -> Result<JsonValue> {
    let region = account.region();
    if region.is_empty() {
        return Err(error!(
            "AWS region is not configured — pass it to AwsAccount::new \
             or set AWS_REGION before issuing a REST-JSON request"
        ));
    }
    let host = format!("{service}.{region}.amazonaws.com");
    let url = build_url(&host, path, query);

    let body_bytes: Vec<u8> = Vec::new();

    let signing_headers = [("host".to_string(), host.clone())];

    let signed = sign_v4(
        account.access_key(),
        account.secret_key(),
        account.session_token(),
        region,
        service,
        method,
        &url,
        &signing_headers,
        &body_bytes,
        SystemTime::now(),
    )?;

    let req_builder = match method {
        "GET" => account.http().get(&url),
        "HEAD" => account.http().head(&url),
        other => {
            return Err(error!(
                "REST-JSON transport currently only supports read methods",
                method = other
            ));
        }
    };
    let mut req = req_builder;
    for h in &signed {
        req = req.header(h.name.as_str(), h.value.as_str());
    }

    let resp = req.send().await.map_err(|e| {
        error!(
            "AWS REST-JSON request failed",
            url = url.as_str(),
            method = method,
            detail = e
        )
    })?;

    let status = resp.status();
    let response_text = resp
        .text()
        .await
        .map_err(|e| error!("Failed to read AWS REST-JSON response body", detail = e))?;

    if !status.is_success() {
        return Err(error!(
            "AWS REST-JSON request returned error status",
            url = url.as_str(),
            status = status.as_u16(),
            body = response_text
        ));
    }

    serde_json::from_str(&response_text).map_err(|e| {
        error!(
            "Failed to parse AWS REST-JSON response",
            detail = e,
            body_preview = response_text.chars().take(200).collect::<String>()
        )
    })
}

fn build_url(host: &str, path: &str, query: &[(String, String)]) -> String {
    let mut url = format!("https://{host}{path}");
    if !query.is_empty() {
        url.push('?');
        for (i, (k, v)) in query.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str(&query_part_encode(k));
            url.push('=');
            url.push_str(&query_part_encode(v));
        }
    }
    url
}

fn query_part_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
