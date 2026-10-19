//! [`RestApi`]: the REST data source, its configuration and its one-off
//! requests. Reads are lowered to URLs in `request`, sent in `fetch`, and
//! assembled into windows in `window` and `page`.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use vantage_api_pool::resilient::{ResilientClient, TransportEvent};
use vantage_core::{Priority, error};

use crate::transport::AuthHeader;

mod builder;
mod config;
mod fetch;
mod page;
mod preview;
mod request;
mod window;

pub use builder::RestApiBuilder;
pub(crate) use config::Order;
pub use config::{FilterStrategy, OrderingParams, PaginationMode, PaginationParams, ResponseShape};

/// The REST shell's `driver_name`.
pub(crate) const REST_DRIVER: &str = "rest-api";

/// REST API backend for Vantage — reads data from HTTP JSON endpoints.
///
/// Each table maps to an API endpoint: `{base_url}/{table_name}`.
/// Response shape, paging, filtering and auth are set through
/// [`RestApi::builder`]. Table writes return `Unsupported` errors; send a
/// write with [`http_request`](Self::http_request). Cheap to clone: the
/// inner [`ResilientClient`] is shared.
#[derive(Clone, Debug)]
pub struct RestApi {
    base_url: String,
    client: ResilientClient,
    auth_header: AuthHeader,
    response_shape: ResponseShape,
    pagination: PaginationParams,
    /// Paging params were configured explicitly, so windows can be fetched
    /// even with no `total_key` — the end of the set shows as a short page.
    paged: bool,
    /// When true, no `_page`/`_limit` query params are appended and
    /// list endpoints are assumed to return the full result set in
    /// one shot. Caller-side requests for page > 1 short-circuit to
    /// an empty result so a perpetual-grid stops paging after the
    /// first chunk. Useful for FastAPI/Pydantic services that treat
    /// unknown query params as strict filters.
    no_pagination: bool,
    /// How non-path eq-conditions are applied — query params vs.
    /// in-memory post-fetch filtering. See [`FilterStrategy`].
    filter_strategy: FilterStrategy,
    /// Response-envelope key carrying the grand total of matching rows
    /// (e.g. `count`). When set, the shell reports an exact count and
    /// advertises the `Window` capability for lazy/scroll loading; when
    /// `None` it falls back to counting fetched rows.
    total_key: Option<String>,
    /// The sort query param, when the API has one. Unset means the API
    /// cannot sort and consumers order fetched rows themselves.
    ordering: Option<OrderingParams>,
    /// Emit `tracing` events for window/count requests.
    debug: bool,
}

impl RestApi {
    /// Create a new REST API pointing at `base_url`, with every builder
    /// default: the [`ResponseShape::default`] envelope (`{ "data": [...] }`)
    /// and JSON Server paging params. For other shapes (bare array,
    /// wrapped-by-table-name) or auth, use [`RestApi::builder`].
    pub fn new(base_url: impl Into<String>) -> Self {
        RestApi::builder(base_url).build()
    }

    /// Start configuring a [`RestApi`] via the builder.
    pub fn builder(base_url: impl Into<String>) -> RestApiBuilder {
        RestApiBuilder::new(base_url.into())
    }

    /// The configured response-envelope total key, if any. When set, the
    /// REST shell can report an exact count and serve `fetch_window`.
    pub fn total_key(&self) -> Option<&str> {
        self.total_key.as_deref()
    }

    /// Whether the shell can serve absolute-offset windows: the API pages
    /// (explicit `pagination_params`, or a `total_key`) and pagination isn't
    /// switched off. Without a total the loader learns the end of the set
    /// from a short page.
    pub(crate) fn serves_windows(&self) -> bool {
        !self.no_pagination && (self.paged || self.total_key.is_some())
    }

    /// The sort query param, when configured.
    pub fn ordering(&self) -> Option<&OrderingParams> {
        self.ordering.as_ref()
    }

    /// What the circuit breaker is doing right now. `None` when the
    /// underlying client has no breaker configured.
    pub fn breaker_state(&self) -> Option<crate::BreakerState> {
        self.client.breaker_state()
    }

    /// The resilient client backing this API — the pool, breaker and
    /// observer a caller outside the read path (e.g. an outbox replaying a
    /// queued write) should share rather than build its own.
    pub fn client(&self) -> &ResilientClient {
        &self.client
    }

    /// The configured base URL requests are joined against.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// A clone whose requests carry the bearer token `source` gives at the
    /// moment each one goes out, in place of the configured `auth`. The
    /// clone shares this API's client — connection pool, parallel cap,
    /// breaker and observer — so one clone per session costs no extra
    /// connections. See [`BearerSource`](crate::BearerSource).
    ///
    /// The client adds an [`auth_refresher`](RestApiBuilder::auth_refresher)'s
    /// header itself, so a clone can't withdraw it: build the shared API
    /// without a refresher when its clones bring their own tokens.
    pub fn with_bearer_source(&self, source: crate::BearerSource) -> Self {
        Self {
            auth_header: AuthHeader::Source(source),
            ..self.clone()
        }
    }

    /// Issue an arbitrary HTTP request against `base_url`/`path`, through
    /// the same resilient client and auth header as reads.
    ///
    /// `headers` are applied after the configured `Authorization` header, so
    /// a caller-supplied header of the same name wins. `body`, when given,
    /// is sent as the JSON request body. The call policy follows
    /// [`vantage_core::Priority::current`], same as a table read. On success,
    /// a non-`GET` method reports [`TransportEvent::WritePushed`] to the
    /// observer.
    ///
    /// A header name or value that HTTP cannot carry (a newline, say) is an
    /// error, returned before anything is sent.
    pub async fn http_request(
        &self,
        method: reqwest::Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&serde_json::Value>,
    ) -> vantage_core::Result<reqwest::Response> {
        let url = join_base_path(&self.base_url, path);

        // A fixed value is checked before anything is sent; a bad one would
        // otherwise surface as a transport error, which an essential call
        // retries forever. A bearer source is asked per attempt, below.
        if let AuthHeader::Fixed(auth) = &self.auth_header {
            HeaderValue::from_str(auth)
                .map_err(|_| error!("Configured Authorization header value is not valid"))?;
        }

        let mut header_map = HeaderMap::new();
        for (name, value) in headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| error!("HTTP header name is not valid", name = *name))?;
            // The value may be a secret, so the error names only the header.
            let header_value = HeaderValue::from_str(value)
                .map_err(|_| error!("HTTP header value is not valid", name = *name))?;
            header_map.insert(header_name, header_value);
        }

        let policy = crate::transport::policy_for(Priority::current());
        let response = self
            .client
            .execute_with(&policy, |http| {
                // `RequestBuilder::headers` replaces same-named entries
                // rather than appending, so a caller header overrides the
                // configured auth instead of riding alongside it on the wire.
                let req = self
                    .auth_header
                    .apply(http.request(method.clone(), &url))
                    .headers(header_map.clone());
                match body {
                    Some(body) => req.json(body),
                    None => req,
                }
            })
            .await
            .map_err(|e| crate::transport::client_error(e, "API request failed", &url))?;

        if method != reqwest::Method::GET {
            self.client.report(TransportEvent::WritePushed);
        }

        Ok(response)
    }
}

/// Join a base URL and a path with exactly one slash between them,
/// regardless of whether either side already carries one.
fn join_base_path(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
