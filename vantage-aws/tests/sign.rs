//! Offline SigV4 correctness check.
//!
//! Drives the hand-rolled signer with the canonical example from AWS's
//! SigV4 documentation and asserts the produced `Authorization` header
//! matches the documented value byte-for-byte.
//!
//! Vector source:
//! https://docs.aws.amazon.com/general/latest/gr/sigv4-signed-request-examples.html
//!
//! These vectors don't change. A regression here means the signer is
//! wrong — every other sign call in the crate is broken too.

use std::time::{Duration, UNIX_EPOCH};

use vantage_aws::__test_support::sign_v4;

const FIXTURE_UNIX_SECS: u64 = 1_440_938_160; // 2015-08-30T12:36:00Z

#[test]
fn aws_canonical_example_get_no_body() {
    let time = UNIX_EPOCH + Duration::from_secs(FIXTURE_UNIX_SECS);

    let signed = sign_v4(
        "AKIDEXAMPLE",
        "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        None,
        "us-east-1",
        "service",
        "GET",
        "https://example.amazonaws.com/?Param2=value2&Param1=value1",
        &[("host".to_string(), "example.amazonaws.com".to_string())],
        b"",
        time,
    )
    .expect("signing should succeed for the canonical fixture");

    let header = |name: &str| {
        signed
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.clone())
    };

    assert_eq!(
        header("authorization").expect("Authorization is added by signer"),
        "AWS4-HMAC-SHA256 \
         Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
         SignedHeaders=host;x-amz-date, \
         Signature=b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500"
    );
    assert_eq!(
        header("x-amz-date").expect("signer adds X-Amz-Date"),
        "20150830T123600Z"
    );
}

#[test]
fn temporary_credentials_carry_session_token() {
    let time = UNIX_EPOCH + Duration::from_secs(FIXTURE_UNIX_SECS);
    let signed = sign_v4(
        "ASIA_TEMP_EXAMPLE",
        "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        Some("FQoGZ_session_token_blob"),
        "eu-west-2",
        "logs",
        "POST",
        "https://logs.eu-west-2.amazonaws.com/",
        &[(
            "host".to_string(),
            "logs.eu-west-2.amazonaws.com".to_string(),
        )],
        b"{}",
        time,
    )
    .expect("signing with session token must succeed");

    let token = signed
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("x-amz-security-token"))
        .map(|h| h.value.clone());
    assert_eq!(token.as_deref(), Some("FQoGZ_session_token_blob"));
}

#[test]
fn body_bytes_change_signature() {
    // Sanity: same headers + URL but different body must produce
    // different signatures. Catches the class of bugs where the body
    // sha256 isn't actually being threaded into the canonical request.
    let time = UNIX_EPOCH + Duration::from_secs(FIXTURE_UNIX_SECS);

    let sign_with = |body: &[u8]| -> String {
        sign_v4(
            "AKIDEXAMPLE",
            "wJalrXUtnFE/secret",
            None,
            "eu-west-2",
            "logs",
            "POST",
            "https://logs.eu-west-2.amazonaws.com/",
            &[(
                "host".to_string(),
                "logs.eu-west-2.amazonaws.com".to_string(),
            )],
            body,
            time,
        )
        .expect("signing succeeds")
        .into_iter()
        .find(|h| h.name.eq_ignore_ascii_case("authorization"))
        .expect("Authorization is always added")
        .value
    };

    let a = sign_with(b"{\"logGroupNamePrefix\":\"a\"}");
    let b = sign_with(b"{\"logGroupNamePrefix\":\"b\"}");
    assert_ne!(a, b, "different bodies must produce different signatures");
}
