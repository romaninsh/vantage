//! AWS Query protocol HTTP transport.
//!
//! Form-encoded POST to `https://{service}.{region}.amazonaws.com/`,
//! signed with SigV4. Request body is `application/x-www-form-urlencoded`
//! (the same Action/Version pair AWS docs all show); response body is
//! XML, returned here as raw text for the caller to parse.

use std::time::SystemTime;

use vantage_core::Result;
use vantage_core::error;

use crate::account::AwsAccount;
use crate::sign::sign_v4;

/// Issue an AWS Query request and return the XML response body as a
/// string. `service` is both the SigV4 service name and the URL
/// hostname segment (e.g. `"iam"`, `"sts"`). `form` is the full set of
/// `(key, value)` pairs that go into the body, including the
/// `Action=` and `Version=` entries the caller is responsible for.
///
/// Global services (IAM, STS in legacy mode, …) live at a single
/// region-less hostname (`iam.amazonaws.com`) and are always signed
/// with `us-east-1`, regardless of the account's configured region.
pub(crate) async fn query_call(
    account: &AwsAccount,
    service: &str,
    form: &[(String, String)],
) -> Result<String> {
    let configured_region = account.region();
    if configured_region.is_empty() && !is_global_service(service) {
        return Err(error!(
            "AWS region is not configured — pass it to AwsAccount::new \
             or set AWS_REGION before calling AwsAccount::from_env"
        ));
    }

    let (host, signing_region) = if is_global_service(service) {
        (format!("{service}.amazonaws.com"), "us-east-1")
    } else {
        (
            format!("{service}.{configured_region}.amazonaws.com"),
            configured_region,
        )
    };
    let url = format!("https://{host}/");

    let body = form_encode(form);
    let body_bytes = body.into_bytes();

    let signing_headers = [
        ("host".to_string(), host.clone()),
        (
            "content-type".to_string(),
            "application/x-www-form-urlencoded; charset=utf-8".to_string(),
        ),
    ];

    let signed = sign_v4(
        account.access_key(),
        account.secret_key(),
        account.session_token(),
        signing_region,
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
        .header(
            "content-type",
            "application/x-www-form-urlencoded; charset=utf-8",
        )
        .body(body_bytes);
    for h in &signed {
        req = req.header(h.name.as_str(), h.value.as_str());
    }

    let resp = req.send().await.map_err(|e| {
        error!(
            "AWS Query request failed",
            url = url.as_str(),
            service = service,
            detail = e
        )
    })?;

    let status = resp.status();
    let response_text = resp
        .text()
        .await
        .map_err(|e| error!("Failed to read AWS Query response body", detail = e))?;

    if !status.is_success() {
        return Err(error!(
            "AWS Query request returned error status",
            service = service,
            status = status.as_u16(),
            body = response_text
        ));
    }

    Ok(response_text)
}

/// AWS services with a single global endpoint (no region in the
/// hostname) signed with `us-east-1`. Add to this list when wiring a
/// new global service.
fn is_global_service(service: &str) -> bool {
    matches!(service, "iam")
}

/// Form-encode `(k, v)` pairs as `application/x-www-form-urlencoded`.
/// Uses the same percent-encoding alphabet SigV4 mandates for the
/// canonical query string, so the body bytes match what was signed.
fn form_encode(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", form_encode_part(k), form_encode_part(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn form_encode_part(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encode_orders_pairs_as_given() {
        let body = form_encode(&[
            ("Action".into(), "ListUsers".into()),
            ("Version".into(), "2010-05-08".into()),
        ]);
        assert_eq!(body, "Action=ListUsers&Version=2010-05-08");
    }

    #[test]
    fn form_encode_escapes_reserved() {
        let body = form_encode(&[("Path".into(), "/admin/team a/".into())]);
        assert_eq!(body, "Path=%2Fadmin%2Fteam%20a%2F");
    }

    #[test]
    fn form_encode_handles_plus_and_amp() {
        let body = form_encode(&[("X".into(), "a+b&c".into())]);
        assert_eq!(body, "X=a%2Bb%26c");
    }
}
