//! [`RestApiBuilder`]: configuring a [`RestApi`].

use std::sync::Arc;

use vantage_api_pool::resilient::TransportObserver;

use super::{FilterStrategy, OrderingParams, PaginationParams, ResponseShape, RestApi};
use crate::transport::AuthHeader;

/// Builder for [`RestApi`]. Lets callers pick a [`ResponseShape`] and
/// override the pagination parameter names.
///
/// ```no_run
/// use vantage_api_client::{RestApi, ResponseShape, PaginationParams};
///
/// // JSONPlaceholder: bare arrays, JSON-Server pagination conventions.
/// let api = RestApi::builder("https://jsonplaceholder.typicode.com")
///     .response_shape(ResponseShape::BareArray)
///     .build();
///
/// // DummyJSON: wrapped-by-table-name, skip-based pagination.
/// let api = RestApi::builder("https://dummyjson.com")
///     .response_shape(ResponseShape::WrappedByTableName)
///     .pagination_params(PaginationParams::skip_limit("skip", "limit"))
///     .build();
/// ```
#[derive(Clone, Debug)]
pub struct RestApiBuilder {
    base_url: String,
    auth_header: AuthHeader,
    response_shape: ResponseShape,
    pagination: PaginationParams,
    /// `pagination_params` was called: the API is known to page.
    paged: bool,
    no_pagination: bool,
    filter_strategy: FilterStrategy,
    total_key: Option<String>,
    ordering: Option<OrderingParams>,
    debug: bool,
    transport: crate::transport::ClientConfig,
}

impl RestApiBuilder {
    pub(super) fn new(base_url: String) -> Self {
        Self {
            base_url,
            auth_header: AuthHeader::default(),
            response_shape: ResponseShape::default(),
            pagination: PaginationParams::default(),
            paged: false,
            no_pagination: false,
            filter_strategy: FilterStrategy::default(),
            total_key: None,
            ordering: None,
            debug: false,
            transport: crate::transport::ClientConfig::default(),
        }
    }

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
        key: impl Into<Arc<str>>,
        observer: Arc<dyn TransportObserver>,
    ) -> Self {
        self.transport.observer = Some((key.into(), observer));
        self
    }

    /// A pre-configured `reqwest::Client` (timeouts, proxies, TLS).
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.transport.http = Some(client);
        self
    }

    /// Set the Authorization header value (e.g. "Bearer `<token>`").
    pub fn auth(mut self, auth: impl Into<String>) -> Self {
        self.auth_header = AuthHeader::new(auth);
        self.transport.auth_refresher = None;
        self
    }

    /// Get the bearer token from `refresher` instead of a fixed header: it
    /// is asked on the first request, and again when the API answers `401`.
    /// A request waits for it, so a refresher may run an interactive
    /// sign-in. Replaces [`auth`](Self::auth).
    pub fn auth_refresher(mut self, refresher: crate::AuthRefresher) -> Self {
        self.auth_header = AuthHeader::default();
        self.transport.auth_refresher = Some(refresher);
        self
    }

    /// Choose how the API wraps its row array. Defaults to
    /// `Wrapped { array_key: "data" }` for backwards compat.
    pub fn response_shape(mut self, shape: ResponseShape) -> Self {
        self.response_shape = shape;
        self
    }

    /// Override the page/limit query parameter names. Default is
    /// `("_page", "_limit")` (JSON Server convention).
    pub fn pagination_params(mut self, pagination: PaginationParams) -> Self {
        self.pagination = pagination;
        self.paged = true;
        self
    }

    /// Disable pagination entirely — no `_page`/`_limit` query
    /// params are appended, and a request for page > 1 is short-
    /// circuited to an empty result. Use this for APIs that don't
    /// paginate (return the full list every call) or that treat
    /// unknown query params as strict filters.
    pub fn no_pagination(mut self) -> Self {
        self.no_pagination = true;
        self
    }

    /// Choose how non-path eq-conditions are applied. Default is
    /// [`FilterStrategy::Query`]; use [`FilterStrategy::Client`] for
    /// APIs that only filter via path segments and reject/ignore unknown
    /// query params (the conditions are then applied in memory).
    pub fn filter_strategy(mut self, strategy: FilterStrategy) -> Self {
        self.filter_strategy = strategy;
        self
    }

    /// Name the response-envelope key carrying the grand total of matching
    /// rows (e.g. `count`). Setting it lets the shell report an exact count,
    /// advertise the `Window` capability for lazy/scroll loading, and put the
    /// total on every fetched page.
    pub fn total_key(mut self, key: impl Into<String>) -> Self {
        self.total_key = Some(key.into());
        self
    }

    /// The API's sort query param (see [`OrderingParams`]). With it set the
    /// vista advertises the `Order` capability and every column is orderable.
    pub fn ordering(mut self, ordering: OrderingParams) -> Self {
        self.ordering = Some(ordering);
        self
    }

    /// Emit `tracing` events for window/count requests.
    pub fn debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Finish the configuration. Builds no HTTP client yet: unless one was
    /// given with [`http_client`](Self::http_client), the default client is
    /// created on the first request.
    pub fn build(self) -> RestApi {
        RestApi {
            base_url: self.base_url,
            client: crate::transport::build_client(self.transport),
            auth_header: self.auth_header,
            response_shape: self.response_shape,
            pagination: self.pagination,
            paged: self.paged,
            no_pagination: self.no_pagination,
            filter_strategy: self.filter_strategy,
            total_key: self.total_key,
            ordering: self.ordering,
            debug: self.debug,
        }
    }
}
