//! REST-XML HTTP transport (S3 etc.).
//!
//! Two halves:
//!   - [`build_request`] turns the operation's `"METHOD path?query"`
//!     target plus the resolved conditions into the wire-ready
//!     `(method, path, query_pairs)`. Path placeholders (`{Bucket}`)
//!     pull their value from conditions; remaining conditions become
//!     query-string params.
//!   - [`restxml_call`] signs and sends the request, returning the raw
//!     XML text.
//!
//! Path-style addressing only — `https://{service}.{region}.amazonaws.com/...`.
//! Virtual-host style (`{bucket}.s3.{region}...`) would need DNS-safe
//! bucket names and additional cross-region routing; v0 stays simple
//! and lets the caller set the region for the bucket they're targeting.
//!
//! S3 always requires `x-amz-content-sha256` to be a signed header
//! with the body's hex sha256. We add it unconditionally for every
//! REST-XML service — others accept it fine, S3 errors without it.

use std::time::SystemTime;

use vantage_core::{Result, error};

use crate::account::AwsAccount;
use crate::condition::AwsCondition;
use crate::sign::sign_v4;

/// SHA256 of the empty body — used as the `x-amz-content-sha256`
/// value for every read-only request we issue (always GET / no body).
const EMPTY_BODY_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Parse a `"METHOD path?query"` target and splice the resolved
/// conditions in: `{Placeholder}` segments take their value from a
/// matching `eq("Placeholder", _)` condition, and any leftover
/// conditions are appended to the query string.
///
/// Returns `(method, path, query_pairs)`. `query_pairs` carry the
/// static query from the target plus any condition-derived params,
/// in target-then-condition order.
#[allow(clippy::type_complexity)]
pub(crate) fn build_request(
    target: &str,
    resolved: &[AwsCondition],
) -> Result<(String, String, Vec<(String, String)>)> {
    let (method, rest) = target.split_once(' ').ok_or_else(|| {
        error!(
            "REST target must be \"METHOD path[?query]\" — got",
            target = target
        )
    })?;
    let method = method.trim().to_ascii_uppercase();
    let (path_template, static_query) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };

    // Pull (field, value) pairs from the resolved conditions. By the
    // time we get here Deferred has been materialised, multi-value
    // sets have errored out.
    let mut params: Vec<(String, String)> = Vec::with_capacity(resolved.len());
    for cond in resolved {
        match cond {
            AwsCondition::Eq { field, value } => {
                params.push((field.clone(), cbor_scalar_to_string(value)));
            }
            AwsCondition::In { field, values } => match values.as_slice() {
                [single] => params.push((field.clone(), cbor_scalar_to_string(single))),
                [] => {
                    return Err(error!(
                        "AwsCondition::In with zero values is not representable",
                        field = field.as_str()
                    ));
                }
                _ => {
                    return Err(error!(
                        "AWS REST APIs don't accept multi-value filters; \
                         resolved condition must collapse to one value",
                        field = field.as_str(),
                        count = values.len()
                    ));
                }
            },
            AwsCondition::Deferred { field, .. } => {
                return Err(error!(
                    "Internal: Deferred condition reached REST builder unresolved",
                    field = field.as_str()
                ));
            }
        }
    }

    // Substitute {Placeholder} segments from the params, dropping each
    // one we consumed so it doesn't reappear in the query string.
    let mut path = String::with_capacity(path_template.len());
    let mut consumed: Vec<String> = Vec::new();
    let mut chars = path_template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut name = String::new();
            for nc in chars.by_ref() {
                if nc == '}' {
                    break;
                }
                name.push(nc);
            }
            let value = params
                .iter()
                .find(|(k, _)| k == &name)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| {
                    error!(
                        "REST path placeholder has no matching condition",
                        placeholder = name.as_str()
                    )
                })?;
            // Path segments are URL-path-encoded — keep `/` literal so
            // multi-segment placeholders (rare, but Lambda allows them)
            // don't double-encode.
            path.push_str(&path_segment_encode(&value));
            consumed.push(name);
        } else {
            path.push(c);
        }
    }

    // Build the query string: static pairs from the target first, then
    // conditions that weren't consumed by path placeholders. Both come
    // through the same encoder so signing matches the wire bytes.
    let mut query_pairs: Vec<(String, String)> = Vec::new();
    if !static_query.is_empty() {
        for kv in static_query.split('&') {
            let (k, v) = match kv.split_once('=') {
                Some((k, v)) => (k.to_string(), v.to_string()),
                None => (kv.to_string(), String::new()),
            };
            query_pairs.push((k, v));
        }
    }
    for (k, v) in params {
        if consumed.contains(&k) {
            continue;
        }
        query_pairs.push((k, v));
    }

    Ok((method, path, query_pairs))
}

/// Issue a signed REST-XML request and return the response body as
/// raw text. Empty body — we're read-only in v0.
pub(crate) async fn restxml_call(
    account: &AwsAccount,
    service: &str,
    method: &str,
    path: &str,
    query: &[(String, String)],
) -> Result<String> {
    let region = account.region();
    if region.is_empty() {
        return Err(error!(
            "AWS region is not configured — pass it to AwsAccount::new \
             or set AWS_REGION before issuing a REST-XML request"
        ));
    }
    let host = format!("{service}.{region}.amazonaws.com");
    let url = build_url(&host, path, query);

    let body_bytes: Vec<u8> = Vec::new();

    let signing_headers = [
        ("host".to_string(), host.clone()),
        (
            "x-amz-content-sha256".to_string(),
            EMPTY_BODY_SHA256.to_string(),
        ),
    ];

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
                "REST-XML transport currently only supports read methods",
                method = other
            ));
        }
    };
    let mut req = req_builder.header("x-amz-content-sha256", EMPTY_BODY_SHA256);
    for h in &signed {
        req = req.header(h.name.as_str(), h.value.as_str());
    }

    let resp = req.send().await.map_err(|e| {
        error!(
            "AWS REST-XML request failed",
            url = url.as_str(),
            method = method,
            detail = e
        )
    })?;

    let status = resp.status();
    let response_text = resp
        .text()
        .await
        .map_err(|e| error!("Failed to read AWS REST-XML response body", detail = e))?;

    if !status.is_success() {
        return Err(error!(
            "AWS REST-XML request returned error status",
            url = url.as_str(),
            status = status.as_u16(),
            body = response_text
        ));
    }

    Ok(response_text)
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

/// Path-segment encoder: keep `/` literal (multi-segment placeholders
/// like `{Key}` may legitimately contain it) and percent-encode the
/// rest of the reserved set per RFC 3986.
fn path_segment_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/');
        if unreserved {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Query-string component encoder — matches the SigV4 alphabet.
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

/// Render a CBOR scalar for use in a path / query string. Matches the
/// shape of `condition::cbor_to_string` — kept local to avoid making
/// that module's helper public.
fn cbor_scalar_to_string(v: &ciborium::Value) -> String {
    match v {
        ciborium::Value::Text(s) => s.clone(),
        ciborium::Value::Integer(i) => {
            let n: i128 = (*i).into();
            n.to_string()
        }
        ciborium::Value::Float(f) => f.to_string(),
        ciborium::Value::Bool(b) => b.to_string(),
        ciborium::Value::Null => String::new(),
        // Compound values shouldn't reach here for REST APIs, but
        // defensively render via JSON so we don't drop data silently.
        other => other
            .deserialized::<serde_json::Value>()
            .map(|v| v.to_string())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::AwsCondition;
    use ciborium::Value as CborValue;

    #[test]
    fn build_request_substitutes_placeholders_from_conditions() {
        let target = "GET /{Bucket}?list-type=2";
        let conds = [AwsCondition::Eq {
            field: "Bucket".into(),
            value: CborValue::from("my-bucket"),
        }];
        let (m, p, q) = build_request(target, &conds).unwrap();
        assert_eq!(m, "GET");
        assert_eq!(p, "/my-bucket");
        assert_eq!(q, vec![("list-type".to_string(), "2".to_string())]);
    }

    #[test]
    fn build_request_pushes_unmatched_conditions_to_query() {
        let target = "GET /{Bucket}?list-type=2";
        let conds = [
            AwsCondition::Eq {
                field: "Bucket".into(),
                value: CborValue::from("foo"),
            },
            AwsCondition::Eq {
                field: "prefix".into(),
                value: CborValue::from("logs/"),
            },
        ];
        let (_m, p, q) = build_request(target, &conds).unwrap();
        assert_eq!(p, "/foo");
        assert_eq!(
            q,
            vec![
                ("list-type".to_string(), "2".to_string()),
                ("prefix".to_string(), "logs/".to_string()),
            ]
        );
    }

    #[test]
    fn build_request_errors_on_missing_placeholder_value() {
        let target = "GET /{Bucket}";
        let err = build_request(target, &[]).unwrap_err();
        assert!(format!("{err}").contains("placeholder"));
    }

    #[test]
    fn build_request_no_query_section_is_fine() {
        let target = "GET /";
        let (m, p, q) = build_request(target, &[]).unwrap();
        assert_eq!(m, "GET");
        assert_eq!(p, "/");
        assert!(q.is_empty());
    }
}
