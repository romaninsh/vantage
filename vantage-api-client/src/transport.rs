//! Glue between this crate and `vantage-api-pool`'s `ResilientClient`:
//! how a datasource's settings become a client, how the `Priority`
//! task-local becomes a call policy, and how a transport error becomes a
//! `VantageError` that keeps the status and the server's message.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use reqwest::RequestBuilder;
use reqwest::header::AUTHORIZATION;
use vantage_api_pool::resilient::{
    AuthRefresher, CallPolicy, ClientError, ResilientClient, TransportObserver,
};
use vantage_core::{Priority, VantageError, error};

/// Where a request's bearer token comes from when it differs per caller:
/// asked each time a request goes out, retries included, and answering
/// the bare token (`Bearer ` is prepended). `None` sends no
/// `Authorization` header on that request.
///
/// Set with `bearer_source` on a builder, or `with_bearer_source` on a
/// built API to get a clone that shares its client but sends its own
/// caller's token, e.g. one clone per signed-in session.
pub type BearerSource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// The `Authorization` header a datasource adds to its requests. Held in
/// its own type so that `Debug` — derived on every struct that carries
/// one — prints `Some("<set>")` instead of leaking the token into a log
/// line.
#[derive(Clone, Default)]
pub(crate) enum AuthHeader {
    /// No header from the datasource (an `auth_refresher` on the client
    /// may still add one).
    #[default]
    None,
    /// The same header value on every request.
    Fixed(String),
    /// A token asked for per request.
    Source(BearerSource),
}

impl AuthHeader {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self::Fixed(value.into())
    }

    /// The value for the request going out now: the fixed value, or
    /// `Bearer <token>` with the source asked at this moment. `None` sends
    /// no header.
    pub(crate) fn value(&self) -> Option<Cow<'_, str>> {
        match self {
            Self::None => None,
            Self::Fixed(value) => Some(Cow::Borrowed(value)),
            Self::Source(source) => source().map(|token| Cow::Owned(format!("Bearer {token}"))),
        }
    }

    /// `req` with this header added, when there is one to send. Call it
    /// inside the request-building closure so a source is asked again on
    /// every attempt.
    pub(crate) fn apply(&self, req: RequestBuilder) -> RequestBuilder {
        match self.value() {
            Some(value) => req.header(AUTHORIZATION, value.as_ref()),
            None => req,
        }
    }

    /// What a debug dump shows in place of the value. A source isn't
    /// asked: a preview shouldn't consume a token.
    pub(crate) fn masked(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Fixed(_) => Some("<set>"),
            Self::Source(_) => Some("<per request>"),
        }
    }
}

impl std::fmt::Debug for AuthHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.masked().fmt(f)
    }
}

/// What both API builders collect for the shared client.
#[derive(Clone)]
pub(crate) struct ClientConfig {
    pub max_parallel: usize,
    pub rate_limit: Option<f64>,
    pub observer: Option<(Arc<str>, Arc<dyn TransportObserver>)>,
    pub http: Option<reqwest::Client>,
    /// `None` keeps the transport's default; `Some(None)` disables the
    /// timeout. Ignored when `http` is supplied.
    pub request_timeout: Option<Option<Duration>>,
    /// Bearer token source, asked on the first request and again on a `401`.
    pub auth_refresher: Option<AuthRefresher>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            max_parallel: 4,
            rate_limit: None,
            observer: None,
            http: None,
            request_timeout: None,
            auth_refresher: None,
        }
    }
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("max_parallel", &self.max_parallel)
            .field("rate_limit", &self.rate_limit)
            .field("observer", &self.observer.as_ref().map(|(k, _)| k))
            .field("http", &self.http.is_some())
            .field("request_timeout", &self.request_timeout)
            .field("auth_refresher", &self.auth_refresher.is_some())
            .finish()
    }
}

pub(crate) fn build_client(cfg: ClientConfig) -> ResilientClient {
    let mut b = ResilientClient::builder()
        .max_parallel(cfg.max_parallel)
        .default_breaker();
    if let Some(rate) = cfg.rate_limit {
        b = b.rate_limit(rate);
    }
    if let Some((key, observer)) = cfg.observer {
        b = b.observer(key, observer);
    }
    match cfg.request_timeout {
        Some(Some(timeout)) => b = b.request_timeout(timeout),
        Some(None) => b = b.no_request_timeout(),
        None => {}
    }
    if let Some(http) = cfg.http {
        b = b.http_client(http);
    }
    if let Some(refresher) = cfg.auth_refresher {
        b = b.bearer_auth(refresher);
    }
    b.build()
}

/// Essential waits retry until cancelled and wait for the breaker's probe;
/// background work makes one attempt and fails fast.
pub(crate) fn policy_for(priority: Priority) -> CallPolicy {
    if priority.is_essential() {
        CallPolicy::essential()
    } else {
        CallPolicy::background()
    }
}

/// How much of the server's error body an error message keeps.
const BODY_IN_ERROR: usize = 500;

pub(crate) fn client_error(e: ClientError, what: &'static str, url: &str) -> VantageError {
    let body = e.body.as_deref().map(|b| {
        let end = b
            .char_indices()
            .nth(BODY_IN_ERROR)
            .map_or(b.len(), |(i, _)| i);
        b[..end].to_string()
    });
    match (e.status(), body) {
        (Some(status), Some(body)) => error!(
            what,
            url = url,
            status = status,
            attempts = e.attempts,
            kind = e.kind_name(),
            body = body
        ),
        (Some(status), None) => error!(
            what,
            url = url,
            status = status,
            attempts = e.attempts,
            kind = e.kind_name()
        ),
        (None, _) => error!(
            what,
            url = url,
            attempts = e.attempts,
            kind = e.kind_name(),
            detail = e.to_string()
        ),
    }
}
