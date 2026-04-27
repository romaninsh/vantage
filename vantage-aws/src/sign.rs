//! Hand-rolled AWS Signature V4 for HTTP requests.
//!
//! Implements the algorithm documented at
//! https://docs.aws.amazon.com/general/latest/gr/sigv4-signing.html for
//! the simple non-streaming, non-presigned case — which is all the
//! JSON-1.1 transport needs.
//!
//! Only the canonical-example fixture from AWS's docs is the
//! correctness backstop (`tests/sign.rs`). Don't extend this file
//! without re-running that test.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use vantage_core::Result;
use vantage_core::error;

type HmacSha256 = Hmac<Sha256>;

/// One signed-header pair returned by `sign_v4`. The caller folds these
/// onto the outgoing request alongside whatever it was already sending.
#[derive(Clone, Debug)]
pub struct SignedHeader {
    pub name: String,
    pub value: String,
}

/// Sign an AWS request, returning the headers SigV4 requires —
/// `Authorization`, `X-Amz-Date`, and (for temporary creds)
/// `X-Amz-Security-Token`.
///
/// `request_headers` are the headers that go on the wire AND get signed.
/// `body` is the raw request body bytes (or empty for GET).
/// `time` is passed in (not read from the wall clock) so test fixtures
/// can pin a specific instant and assert byte-equivalence.
#[allow(clippy::too_many_arguments)]
pub fn sign_v4(
    access_key: &str,
    secret_key: &str,
    session_token: Option<&str>,
    region: &str,
    service: &str,
    method: &str,
    url: &str,
    request_headers: &[(String, String)],
    body: &[u8],
    time: SystemTime,
) -> Result<Vec<SignedHeader>> {
    let (date, datetime) = format_amz_date(time)?;
    let credential_scope = format!("{date}/{region}/{service}/aws4_request");

    // 1. Canonical request.
    //    The headers we sign must include x-amz-date — we add it
    //    ourselves rather than asking the caller to.
    let mut signed_headers: Vec<(String, String)> = request_headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    signed_headers.push(("x-amz-date".to_string(), datetime.clone()));
    if let Some(tok) = session_token {
        signed_headers.push(("x-amz-security-token".to_string(), tok.to_string()));
    }
    signed_headers.sort_by(|a, b| a.0.cmp(&b.0));

    let canonical_headers: String = signed_headers
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect();
    let signed_headers_list: String = signed_headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let (path, query) = parse_url_path_query(url)?;
    let body_sha = hex::encode(Sha256::digest(body));

    let canonical_request = format!(
        "{method}\n{path}\n{query}\n{canonical_headers}\n{signed_headers_list}\n{body_sha}",
        method = method.to_ascii_uppercase(),
    );

    // 2. String to sign.
    let crq_hash = hex::encode(Sha256::digest(canonical_request.as_bytes()));
    let string_to_sign = format!("AWS4-HMAC-SHA256\n{datetime}\n{credential_scope}\n{crq_hash}");

    // 3. Signing key = HMAC chain over (date, region, service, "aws4_request").
    let k_secret = format!("AWS4{secret_key}");
    let k_date = hmac_sha256(k_secret.as_bytes(), date.as_bytes())?;
    let k_region = hmac_sha256(&k_date, region.as_bytes())?;
    let k_service = hmac_sha256(&k_region, service.as_bytes())?;
    let k_signing = hmac_sha256(&k_service, b"aws4_request")?;

    // 4. Signature.
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes())?);

    // 5. Authorization header.
    let authorization = format!(
        "AWS4-HMAC-SHA256 \
         Credential={access_key}/{credential_scope}, \
         SignedHeaders={signed_headers_list}, \
         Signature={signature}"
    );

    let mut out = vec![
        SignedHeader {
            name: "Authorization".into(),
            value: authorization,
        },
        SignedHeader {
            name: "X-Amz-Date".into(),
            value: datetime,
        },
    ];
    if let Some(tok) = session_token {
        out.push(SignedHeader {
            name: "X-Amz-Security-Token".into(),
            value: tok.to_string(),
        });
    }
    Ok(out)
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|e| error!("HMAC key length invalid", detail = e))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

/// Returns `(YYYYMMDD, YYYYMMDDTHHMMSSZ)`.
fn format_amz_date(time: SystemTime) -> Result<(String, String)> {
    let secs = time
        .duration_since(UNIX_EPOCH)
        .map_err(|e| error!("System time before unix epoch", detail = e))?
        .as_secs() as i64;

    // Hand-roll civil time from epoch — keeps us off `chrono`/`time` for one fn.
    let (y, mo, d, h, mi, s) = epoch_to_utc(secs);
    let date = format!("{y:04}{mo:02}{d:02}");
    let datetime = format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z");
    Ok((date, datetime))
}

/// Convert UNIX seconds to `(year, month, day, hour, minute, second)` UTC.
/// Howard Hinnant's days_from_civil algorithm, inverted.
fn epoch_to_utc(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400) as u32;
    let h = time_of_day / 3600;
    let mi = (time_of_day % 3600) / 60;
    let s = time_of_day % 60;

    // Days from 1970-01-01 → civil date (Hinnant).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y, m, d, h, mi, s)
}

/// Pull path + canonical query string out of a URL. Both are SigV4
/// canonical-form (path stays as-is, query keys are sorted, values are
/// percent-encoded).
fn parse_url_path_query(url: &str) -> Result<(String, String)> {
    let after_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .ok_or_else(|| error!("URL missing scheme", url = url))?;
    let path_and_query = after_scheme
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let (path_raw, query_raw) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path_and_query, ""),
    };

    let path = if path_raw.is_empty() {
        "/".to_string()
    } else {
        format!("/{path_raw}")
    };

    // Canonicalise the query string: split into (key, value) pairs,
    // percent-encode each side per RFC 3986 (unreserved alphabet),
    // sort by encoded key+value, rejoin with `&`.
    let mut pairs: Vec<(String, String)> = Vec::new();
    if !query_raw.is_empty() {
        for kv in query_raw.split('&') {
            let (k, v) = match kv.split_once('=') {
                Some((k, v)) => (k, v),
                None => (kv, ""),
            };
            pairs.push((sigv4_encode(k), sigv4_encode(v)));
        }
        pairs.sort();
    }
    let query = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    Ok((path, query))
}

/// Percent-encode per the SigV4 unreserved alphabet
/// (`A-Z a-z 0-9 - _ . ~`). Everything else, including `/` in query
/// values, becomes `%XX`.
fn sigv4_encode(s: &str) -> String {
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
    fn epoch_round_trip_known_dates() {
        // 2015-08-30T12:36:00Z = 1_440_938_160
        assert_eq!(epoch_to_utc(1_440_938_160), (2015, 8, 30, 12, 36, 0));
        // 1970-01-01T00:00:00Z
        assert_eq!(epoch_to_utc(0), (1970, 1, 1, 0, 0, 0));
        // 2000-02-29T12:00:00Z (leap year via 400-year rule)
        assert_eq!(epoch_to_utc(951_825_600), (2000, 2, 29, 12, 0, 0));
    }

    #[test]
    fn url_with_no_path_yields_root() {
        let (p, q) = parse_url_path_query("https://logs.eu-west-2.amazonaws.com/").unwrap();
        assert_eq!(p, "/");
        assert_eq!(q, "");
    }

    #[test]
    fn query_string_is_canonicalised() {
        // Order of keys gets sorted, values are passed through.
        let (_p, q) =
            parse_url_path_query("https://example.com/?Param2=value2&Param1=value1").unwrap();
        assert_eq!(q, "Param1=value1&Param2=value2");
    }
}
