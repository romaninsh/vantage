//! Glue between this crate and `vantage-api-pool`'s `ResilientClient`:
//! how a datasource's settings become a client, how the `Priority`
//! task-local becomes a call policy, and how a transport error becomes a
//! `VantageError` that keeps the status and the server's message.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use reqwest::RequestBuilder;
use reqwest::header::{HeaderName, HeaderValue};
use vantage_api_pool::resilient::{
    AuthRefresher, CallPolicy, ClientError, ResilientClient, TransportObserver,
};
use vantage_core::{Priority, VantageError, error};

use crate::redact::{MASK, Redaction};

/// Where a request's bearer token comes from when it differs per caller:
/// asked each time a request goes out, retries included, and answering
/// the bare token (`Bearer ` is prepended). `None` sends no
/// `Authorization` header on that request.
///
/// Set with `bearer_source` on a builder, or `with_bearer_source` on a
/// built API to get a clone that shares its client but sends its own
/// caller's token, e.g. one clone per signed-in session.
pub type BearerSource = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// The auth header a datasource adds to its requests: `Authorization`, or
/// the one an API key names. Held in
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
    /// A key sent verbatim under a header of its own (`X-API-Key`) in
    /// place of `Authorization`.
    Named { name: String, value: String },
}

impl AuthHeader {
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self::Fixed(value.into())
    }

    pub(crate) fn named(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Named {
            name: name.into(),
            value: value.into(),
        }
    }

    /// The header the credential goes out under.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Named { name, .. } => name,
            _ => "Authorization",
        }
    }

    /// The value for the request going out now: the fixed value, or
    /// `Bearer <token>` with the source asked at this moment. `None` sends
    /// no header.
    pub(crate) fn value(&self) -> Option<Cow<'_, str>> {
        match self {
            Self::None => None,
            Self::Fixed(value) | Self::Named { value, .. } => Some(Cow::Borrowed(value)),
            Self::Source(source) => source().map(|token| Cow::Owned(format!("Bearer {token}"))),
        }
    }

    /// A fixed name and value that HTTP can't carry, as an error. Checked
    /// before anything is sent: a bad header would otherwise surface as a
    /// transport error, which an essential call retries forever. A bearer
    /// source is asked per attempt, so it isn't checked here.
    pub(crate) fn validate(&self) -> vantage_core::Result<()> {
        if let Self::Named { name, .. } = self {
            HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                error!(
                    "Configured auth header name is not valid",
                    name = name.as_str()
                )
            })?;
        }
        if let Self::Fixed(value) | Self::Named { value, .. } = self {
            HeaderValue::from_str(value).map_err(|_| {
                error!(
                    "Configured auth header value is not valid",
                    name = self.name()
                )
            })?;
        }
        Ok(())
    }

    /// `req` with this header added, when there is one to send. Call it
    /// inside the request-building closure so a source is asked again on
    /// every attempt. The value that goes out is noted on `redaction`, so
    /// the token an error masks is the one that was sent.
    /// The value is marked sensitive, so reqwest and hyper never print it.
    pub(crate) fn apply(&self, req: RequestBuilder, redaction: &Redaction) -> RequestBuilder {
        let Some(value) = self.value() else {
            return req;
        };
        redaction.sent(&value);
        match HeaderValue::from_str(&value) {
            Ok(mut header) => {
                header.set_sensitive(true);
                req.header(self.name(), header)
            }
            // A bearer source's token isn't validated up front; let reqwest
            // report it as the builder error it is.
            Err(_) => req.header(self.name(), value.as_ref()),
        }
    }

    /// The credential goes out under a header reqwest doesn't strip when a
    /// redirect leaves the origin.
    fn is_named(&self) -> bool {
        matches!(self, Self::Named { .. })
    }

    /// What a debug dump shows in place of the value. A source isn't
    /// asked: a preview shouldn't consume a token.
    pub(crate) fn masked(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Fixed(_) | Self::Named { .. } => Some("<set>"),
            Self::Source(_) => Some("<per request>"),
        }
    }
}

impl std::fmt::Debug for AuthHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.masked().fmt(f)
    }
}

/// A credential sent as a query parameter on every request. `Debug` keeps
/// the value out of log lines.
#[derive(Clone)]
pub(crate) struct QueryAuth {
    pub(crate) param: String,
    pub(crate) value: String,
}

impl std::fmt::Debug for QueryAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryAuth")
            .field("param", &self.param)
            .field("value", &MASK)
            .finish()
    }
}

impl QueryAuth {
    /// `url` with `param=value` appended: the URL that is sent.
    pub(crate) fn sent(&self, url: &str) -> String {
        self.append(url, &urlencoding::encode(&self.value))
    }

    /// `url` as a log line or an error shows it: `param=***`.
    pub(crate) fn shown(&self, url: &str) -> String {
        self.append(url, MASK)
    }

    fn append(&self, url: &str, value: &str) -> String {
        let joiner = if url.contains('?') { '&' } else { '?' };
        format!("{url}{joiner}{}={value}", urlencoding::encode(&self.param))
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

/// The builder methods `RestApiBuilder` and `GraphqlApiBuilder` share,
/// expanded inside each one's `impl` block. The builder needs a
/// `transport: ClientConfig` and an `auth_header: AuthHeader` field.
macro_rules! client_builder_methods {
    () => {
        /// Concurrent requests to this API at most (default 4).
        pub fn max_parallel(mut self, n: usize) -> Self {
            self.transport.max_parallel = n.max(1);
            self
        }

        /// Requests per second to this API at most.
        pub fn rate_limit(mut self, per_second: f64) -> Self {
            self.transport.rate_limit = Some(per_second);
            self
        }

        /// Give up on one request after `timeout` (default 30 s). Ignored when a
        /// client is supplied with [`http_client`](Self::http_client).
        pub fn request_timeout(mut self, timeout: std::time::Duration) -> Self {
            self.transport.request_timeout = Some(Some(timeout));
            self
        }

        /// Never time a request out. Ignored when a client is supplied.
        pub fn no_request_timeout(mut self) -> Self {
            self.transport.request_timeout = Some(None);
            self
        }

        /// Report every attempt, retry and breaker transition under `key`
        /// (the datasource name).
        pub fn observer(
            mut self,
            key: impl Into<std::sync::Arc<str>>,
            observer: std::sync::Arc<dyn $crate::TransportObserver>,
        ) -> Self {
            self.transport.observer = Some((key.into(), observer));
            self
        }

        /// A pre-configured `reqwest::Client` (timeouts, proxies, TLS), used as
        /// is. With [`header_auth`](Self::header_auth), build it with
        /// [`same_origin_redirects`](crate::same_origin_redirects) (or no
        /// redirects): reqwest forwards a custom header on a redirect to
        /// another host.
        pub fn http_client(mut self, client: reqwest::Client) -> Self {
            self.transport.http = Some(client);
            self
        }

        fn fixed_auth(mut self, header: $crate::transport::AuthHeader) -> Self {
            self.auth_header = header;
            self.transport.auth_refresher = None;
            self
        }

        /// Get the bearer token from `refresher` instead of a fixed header: it
        /// is asked on the first request, and again when the API answers `401`.
        /// A request waits for it, so a refresher may run an interactive
        /// sign-in. Replaces [`auth`](Self::auth).
        pub fn auth_refresher(mut self, refresher: $crate::AuthRefresher) -> Self {
            self.auth_header = $crate::transport::AuthHeader::default();
            self.transport.auth_refresher = Some(refresher);
            self
        }

        /// Ask `source` for the bearer token on every request, retries
        /// included; see [`BearerSource`](crate::BearerSource). Replaces
        /// [`auth`](Self::auth) and [`auth_refresher`](Self::auth_refresher).
        pub fn bearer_source(mut self, source: $crate::BearerSource) -> Self {
            self.auth_header = $crate::transport::AuthHeader::Source(source);
            self.transport.auth_refresher = None;
            self
        }
    };
}

pub(crate) use client_builder_methods;

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

/// How many redirects [`same_origin_redirects`] follows, as reqwest's
/// default policy does.
const MAX_REDIRECTS: usize = 10;

/// A redirect policy that follows redirects within the request's origin
/// (scheme, host and port) and stops at one leading elsewhere: the `3xx`
/// comes back as the response, and an API call fails with its status.
///
/// reqwest drops `Authorization`, `Cookie` and `Proxy-Authorization` on a
/// cross-origin redirect but forwards every other header, so a key sent
/// with `header_auth` would reach the other host. The client this crate
/// builds uses this policy whenever `header_auth` is set; give it to a
/// client you pass to `http_client` yourself:
///
/// ```no_run
/// use vantage_api_client::{RestApi, same_origin_redirects};
///
/// let http = reqwest::Client::builder()
///     .redirect(same_origin_redirects())
///     .build()
///     .unwrap();
/// let api = RestApi::builder("https://api.example.com")
///     .http_client(http)
///     .header_auth("X-API-Key", "k-123")
///     .build();
/// ```
pub fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            return attempt.error("too many redirects");
        }
        let same_origin = attempt
            .previous()
            .first()
            .is_none_or(|first| first.origin() == attempt.url().origin());
        if same_origin {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

/// The client for one API. `auth` decides the redirect policy of the
/// client built here; a supplied client keeps its own.
pub(crate) fn build_client(cfg: ClientConfig, auth: &AuthHeader) -> ResilientClient {
    let mut b = ResilientClient::builder()
        .max_parallel(cfg.max_parallel)
        .default_breaker();
    if auth.is_named() {
        b = b.configure_http(|http| http.redirect(same_origin_redirects()));
    }
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

/// A transport failure as an error. `url`, the server's body and the
/// transport's own text all pass through `redaction`: a client error can
/// quote the request URL.
pub(crate) fn client_error(
    e: ClientError,
    what: &'static str,
    url: &str,
    redaction: &Redaction,
) -> VantageError {
    let url = redaction.url(url);
    let url = url.as_str();
    let body = e.body.as_deref().map(|b| {
        let end = b
            .char_indices()
            .nth(BODY_IN_ERROR)
            .map_or(b.len(), |(i, _)| i);
        redaction.text(&b[..end])
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
            detail = redaction.text(&e.to_string())
        ),
    }
}
