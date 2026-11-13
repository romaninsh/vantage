//! [`RestApiBuilder`]: configuring a [`RestApi`].

use super::{FilterStrategy, OrderingParams, PaginationParams, ResponseShape, RestApi};
use crate::transport::{AuthHeader, QueryAuth, client_builder_methods};

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
    query_auth: Option<QueryAuth>,
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
            query_auth: None,
            transport: crate::transport::ClientConfig::default(),
        }
    }

    client_builder_methods!();

    /// Set the Authorization header value (e.g. "Bearer `<token>`").
    pub fn auth(self, auth: impl Into<String>) -> Self {
        self.fixed_auth(AuthHeader::new(auth))
    }

    /// Send `value` verbatim as the `name` header (`X-API-Key`), for APIs
    /// that take their key outside `Authorization`. Replaces
    /// [`auth`](Self::auth); logs, errors and previews mask the value.
    ///
    /// A redirect to another origin is not followed, so the key never
    /// leaves the API's host; the request fails with the `3xx` status. A
    /// client passed to [`http_client`](Self::http_client) has to do this
    /// itself, see [`same_origin_redirects`](crate::same_origin_redirects).
    pub fn header_auth(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.fixed_auth(AuthHeader::named(name, value))
    }

    /// Send `param=value` on every request, reads and inserts alike, for
    /// APIs that take their key in the query string (GA4's `api_secret`).
    /// The real value goes on the wire; logs, errors and previews show
    /// `param=***`.
    pub fn query_auth(mut self, param: impl Into<String>, value: impl Into<String>) -> Self {
        self.query_auth = Some(QueryAuth {
            param: param.into(),
            value: value.into(),
        });
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
            client: crate::transport::build_client(self.transport, &self.auth_header),
            auth_header: self.auth_header,
            response_shape: self.response_shape,
            pagination: self.pagination,
            paged: self.paged,
            no_pagination: self.no_pagination,
            filter_strategy: self.filter_strategy,
            total_key: self.total_key,
            ordering: self.ordering,
            debug: self.debug,
            query_auth: self.query_auth,
            insert: None,
            readable: true,
        }
    }
}
