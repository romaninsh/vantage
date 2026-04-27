//! `AwsAccount` — account-wide credentials handle.
//!
//! Holds the access key, secret key, and region. Cheap to clone (everything
//! lives behind an `Arc`). Used directly as the `TableSource` for JSON-1.1
//! tables — see `crate::json1` for the protocol impl. The per-operation
//! configuration (service, operation target, response array key) lives in
//! the table name, formatted as `array_key:service/target`.

use std::sync::Arc;

use vantage_core::Result;
use vantage_core::error;

#[derive(Clone)]
pub struct AwsAccount {
    inner: Arc<Inner>,
}

struct Inner {
    access_key: String,
    secret_key: String,
    session_token: Option<String>,
    region: String,
    http: reqwest::Client,
}

impl AwsAccount {
    /// Construct from explicit static credentials.
    pub fn new(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        region: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                access_key: access_key.into(),
                secret_key: secret_key.into(),
                session_token: None,
                region: region.into(),
                http: reqwest::Client::new(),
            }),
        }
    }

    /// Construct from `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`,
    /// optional `AWS_SESSION_TOKEN`, and `AWS_REGION` environment
    /// variables. The credential chain (profiles, IMDS, SSO) is out of
    /// scope for v0 — set the env vars yourself or call `new`.
    pub fn from_env() -> Result<Self> {
        let access_key = std::env::var("AWS_ACCESS_KEY_ID")
            .map_err(|_| error!("AWS_ACCESS_KEY_ID not set"))?;
        let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
            .map_err(|_| error!("AWS_SECRET_ACCESS_KEY not set"))?;
        let region = std::env::var("AWS_REGION")
            .map_err(|_| error!("AWS_REGION not set"))?;
        let session_token = std::env::var("AWS_SESSION_TOKEN").ok();

        Ok(Self {
            inner: Arc::new(Inner {
                access_key,
                secret_key,
                session_token,
                region,
                http: reqwest::Client::new(),
            }),
        })
    }

    pub(crate) fn region(&self) -> &str {
        &self.inner.region
    }

    pub(crate) fn access_key(&self) -> &str {
        &self.inner.access_key
    }

    pub(crate) fn secret_key(&self) -> &str {
        &self.inner.secret_key
    }

    pub(crate) fn session_token(&self) -> Option<&str> {
        self.inner.session_token.as_deref()
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.inner.http
    }
}

impl std::fmt::Debug for AwsAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsAccount")
            .field("region", &self.inner.region)
            .field("access_key", &"<redacted>")
            .field("secret_key", &"<redacted>")
            .field("session_token", &self.inner.session_token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}
