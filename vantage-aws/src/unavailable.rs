//! Which AWS failures are worth retrying.
//!
//! AWS reports throttling as a 400 with an error code in the body, so the
//! status alone can't tell a throttled call from a refused one. The code
//! list follows the AWS SDKs' throttling and transient classes, minus
//! `LimitExceededException`, which several services use for a hard quota.

use vantage_api_pool::{is_retryable_status, is_transient_reqwest_error};
use vantage_core::VantageError;

const RETRYABLE_CODES: &[&str] = &[
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottled",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
    "RequestTimeout",
    "RequestTimeoutException",
];

/// The AWS error code of a failed response: `<Code>` in XML bodies,
/// `__type` / `code` in JSON bodies (with any `namespace#` prefix dropped).
fn error_code(body: &str) -> Option<String> {
    if let Some(start) = body.find("<Code>") {
        let rest = &body[start + "<Code>".len()..];
        let end = rest.find("</Code>")?;
        return Some(rest[..end].trim().to_string());
    }
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let raw = ["__type", "code", "Code"]
        .iter()
        .find_map(|k| json.get(*k).and_then(|v| v.as_str()))?;
    Some(raw.rsplit('#').next().unwrap_or(raw).to_string())
}

/// Mark an error-status response `Unavailable` when its status is
/// retryable or it carries a throttling / timeout error code.
pub(crate) fn classify_status(err: VantageError, status: u16, body: &str) -> VantageError {
    let retryable_status = is_retryable_status(status);
    let retryable_code =
        error_code(body).is_some_and(|code| RETRYABLE_CODES.contains(&code.as_str()));
    if retryable_status || retryable_code {
        err.mark_unavailable()
    } else {
        err
    }
}

/// A request that got no response (connect, TLS, timeout, body read) is
/// `Unavailable`; one reqwest refused to build is a bug and stays as is.
pub(crate) fn classify_send(err: VantageError, source: &reqwest::Error) -> VantageError {
    if is_transient_reqwest_error(source) {
        err.mark_unavailable()
    } else {
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_core::error;

    #[test]
    fn throttling_400_is_unavailable_and_conditional_check_is_not() {
        let throttled = r#"{"__type":"com.amazonaws.dynamodb.v20120810#ProvisionedThroughputExceededException","message":"slow down"}"#;
        assert!(classify_status(error!("x"), 400, throttled).is_unavailable());

        let xml = "<ErrorResponse><Error><Code>Throttling</Code></Error></ErrorResponse>";
        assert!(classify_status(error!("x"), 400, xml).is_unavailable());

        let refused =
            r#"{"__type":"com.amazonaws.dynamodb.v20120810#ConditionalCheckFailedException"}"#;
        assert!(!classify_status(error!("x"), 400, refused).is_unavailable());
        let invalid = r#"{"__type":"com.amazon.coral.validate#ValidationException"}"#;
        assert!(!classify_status(error!("x"), 400, invalid).is_unavailable());

        assert!(classify_status(error!("x"), 503, "").is_unavailable());
    }
}
