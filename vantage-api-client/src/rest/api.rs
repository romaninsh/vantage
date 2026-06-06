use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_table::pagination::Pagination;
use vantage_types::Record;

/// How the API wraps its row array in the response body.
///
/// Most public APIs use one of these three shapes; the legacy vantage
/// "wrapped under `data`" shape is `Wrapped { array_key: "data" }`.
#[derive(Clone, Debug)]
pub enum ResponseShape {
    /// Body is a bare JSON array of records.
    /// Example: `GET /users` → `[ {…}, {…} ]`. JSONPlaceholder, GitHub, etc.
    BareArray,

    /// Body is a JSON object with the array under a fixed key.
    /// Example: `GET /users` → `{ "data": [ … ] }`.
    Wrapped { array_key: String },

    /// Body is a JSON object with the array under a key matching the
    /// table name. Example (DummyJSON):
    /// `GET /products` → `{ "products": [ … ], "total": …, "skip": …, "limit": … }`.
    WrappedByTableName,
}

impl Default for ResponseShape {
    /// Default matches the legacy 0.1.x shape: `{ "data": [...] }`.
    fn default() -> Self {
        ResponseShape::Wrapped {
            array_key: "data".to_string(),
        }
    }
}

/// Names of the page/limit query parameters the API expects.
///
/// Defaults to `("_page", "_limit")` — the JSON Server convention used
/// by JSONPlaceholder. DummyJSON uses `("skip", "limit")` (in items not
/// pages). Customise via `RestApiBuilder::pagination_params`.
#[derive(Clone, Debug)]
pub struct PaginationParams {
    pub page: String,
    pub limit: String,
    /// If true, the page parameter is sent as a *0-based item offset*
    /// (`skip`) instead of a 1-based page index. DummyJSON-style.
    pub skip_based: bool,
}

impl PaginationParams {
    pub fn page_limit(page: impl Into<String>, limit: impl Into<String>) -> Self {
        Self {
            page: page.into(),
            limit: limit.into(),
            skip_based: false,
        }
    }

    pub fn skip_limit(skip: impl Into<String>, limit: impl Into<String>) -> Self {
        Self {
            page: skip.into(),
            limit: limit.into(),
            skip_based: true,
        }
    }
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self::page_limit("_page", "_limit")
    }
}

/// REST API backend for Vantage — reads data from HTTP JSON endpoints.
///
/// Each table maps to an API endpoint: `{base_url}/{table_name}`.
/// Response shape is configurable via [`RestApi::builder`]; see
/// [`ResponseShape`] for the supported variants.
///
/// Currently read-only — write operations return errors.
/// How a table's conditions are applied to a request.
///
/// URL `{placeholder}` path segments are always filled from matching
/// eq-conditions regardless of strategy; this governs what happens to
/// the *remaining* (non-path) eq-conditions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterStrategy {
    /// Append remaining eq-conditions as `?field=value` query params
    /// (JSON-Server semantics). The default.
    #[default]
    Query,
    /// Apply remaining eq-conditions as in-memory row filters after the
    /// fetch, never as query params. For APIs whose only server-side
    /// filters are path segments and that reject (or ignore) unknown
    /// query params — e.g. the Mercury control-API, whose CLI likewise
    /// filters version/env client-side after fetching by product path.
    Client,
}

#[derive(Clone, Debug)]
pub struct RestApi {
    base_url: String,
    client: reqwest::Client,
    pub(crate) auth_header: Option<String>,
    response_shape: ResponseShape,
    pagination: PaginationParams,
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
}

impl RestApi {
    /// Create a new REST API pointing at `base_url`. Uses the legacy
    /// default response shape (`{ "data": [...] }`). For other shapes
    /// (bare array, wrapped-by-table-name) use [`RestApi::builder`].
    pub fn new(base_url: impl Into<String>) -> Self {
        RestApi::builder(base_url).build()
    }

    /// Start configuring a [`RestApi`] via the builder.
    pub fn builder(base_url: impl Into<String>) -> RestApiBuilder {
        RestApiBuilder::new(base_url.into())
    }

    /// Set the Authorization header value (e.g. "Bearer `<token>`").
    /// Provided for backwards compatibility — prefer
    /// `RestApi::builder(...).auth(...)`.
    pub fn with_auth(mut self, auth: impl Into<String>) -> Self {
        self.auth_header = Some(auth.into());
        self
    }

    /// Build the endpoint path for `table_name`, substituting any
    /// `{placeholder}` segments from matching eq-conditions.
    ///
    /// Returns the absolute URL up to (but excluding) the query string,
    /// alongside the indices of conditions consumed by the substitution
    /// — those are dropped from the query string by `build_query_string`.
    ///
    /// Tables that don't use templates (no `{}` in the name) pass
    /// through unchanged and consume no conditions.
    fn endpoint_url(
        &self,
        table_name: &str,
        conditions: &[&Expression<CborValue>],
    ) -> Result<(String, Vec<usize>)> {
        let mut consumed = Vec::new();
        let mut path = String::with_capacity(table_name.len());
        let mut rest = table_name;
        while let Some(open) = rest.find('{') {
            path.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            let close = after.find('}').ok_or_else(|| {
                error!(
                    "Unclosed `{` in table name URI template",
                    table_name = table_name
                )
            })?;
            let placeholder = &after[..close];
            let (idx, value) = conditions
                .iter()
                .enumerate()
                .find_map(|(i, cond)| {
                    if consumed.contains(&i) {
                        return None;
                    }
                    let (field, value) = crate::condition_to_query_param(cond)?;
                    (field == placeholder).then_some((i, value))
                })
                .ok_or_else(|| {
                    error!(
                        "No eq-condition provided for URI placeholder",
                        placeholder = placeholder,
                        table_name = table_name
                    )
                })?;
            consumed.push(idx);
            path.push_str(&urlencode(&value));
            rest = &after[close + 1..];
        }
        path.push_str(rest);
        Ok((format!("{}/{}", self.base_url, path), consumed))
    }

    /// Build the combined query-string from pagination + conditions.
    /// `consumed` lists condition indices already baked into the URI
    /// path; those don't appear in the query string. Conditions that
    /// don't peel cleanly into eq pairs are skipped — same "best effort"
    /// stance as before.
    fn build_query_string(
        &self,
        pagination: Option<&Pagination>,
        conditions: &[&Expression<CborValue>],
        consumed: &[usize],
    ) -> String {
        let mut params: Vec<(String, String)> = Vec::new();

        // Pagination first — matches the order users see in the URL bar.
        // When `no_pagination` is set the API doesn't accept page/limit
        // query params (and may treat them as strict filters that
        // return empty), so we leave them off.
        if !self.no_pagination
            && let Some(p) = pagination
        {
            let page_value = if self.pagination.skip_based {
                p.skip().to_string()
            } else {
                p.get_page().to_string()
            };
            params.push((self.pagination.page.clone(), page_value));
            params.push((self.pagination.limit.clone(), p.limit().to_string()));
        }

        // Conditions: each `eq` becomes `?field=value`. Multiple
        // conditions AND together (JSON Server semantics).
        for (i, cond) in conditions.iter().enumerate() {
            if consumed.contains(&i) {
                continue;
            }
            if let Some((field, value)) = crate::condition_to_query_param(cond) {
                params.push((field, value));
            }
        }

        if params.is_empty() {
            return String::new();
        }
        let mut s = String::from("?");
        for (i, (k, v)) in params.iter().enumerate() {
            if i > 0 {
                s.push('&');
            }
            // Minimal URL encoding — we encode `&` and `=` and spaces
            // because those break the query format. Anything else
            // passes through; the JSON Server convention is permissive.
            s.push_str(&urlencode(k));
            s.push('=');
            s.push_str(&urlencode(v));
        }
        s
    }

    /// Fetch data from the API endpoint and return parsed records.
    ///
    /// `id_field` selects which JSON field is treated as the record ID;
    /// if `None`, row indices are used. `pagination` and `conditions`
    /// are pushed into the URL query string — eq-conditions become
    /// `?field=value`. Conditions that can't be peeled into a simple
    /// eq are silently skipped (caller-side filtering still applies if
    /// needed).
    pub(crate) async fn fetch_records<'a>(
        &self,
        table_name: &str,
        id_field: Option<&str>,
        pagination: Option<&Pagination>,
        conditions: impl IntoIterator<Item = &'a Expression<CborValue>>,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        // Non-paginating endpoints return the whole list on page 1; a
        // page-2 fetch would just re-deliver the same rows and the
        // perpetual grid would never mark itself exhausted. Short-
        // circuit page > 1 to empty so the grid sees the chunk shrink
        // and stops asking for more.
        if self.no_pagination
            && let Some(p) = pagination
            && p.get_page() > 1
        {
            return Ok(IndexMap::new());
        }

        // Conditions may carry `DeferredFn` values — typically from
        // `related_in_condition` for `with_one`-style traversals where
        // the FK lives in a parent record we haven't fetched yet.
        // Resolve them once, up front, so the rest of the pipeline
        // sees only sync, peelable scalars.
        let raw: Vec<&Expression<CborValue>> = conditions.into_iter().collect();
        let mut resolved: Vec<Expression<CborValue>> = Vec::with_capacity(raw.len());
        for cond in raw {
            resolved.push(resolve_deferreds(cond.clone()).await?);
        }
        let conds: Vec<&Expression<CborValue>> = resolved.iter().collect();
        let (endpoint, consumed) = self.endpoint_url(table_name, &conds)?;

        // Under `FilterStrategy::Client`, non-path eq-conditions are
        // applied to the fetched rows in memory rather than sent as query
        // params (the API rejects/ignores unknown params). Collect them,
        // and keep them out of the query string by marking every
        // condition as consumed for query-building purposes.
        let (query_consumed, client_filters): (Vec<usize>, Vec<(String, String)>) =
            if self.filter_strategy == FilterStrategy::Client {
                let filters = conds
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !consumed.contains(i))
                    .filter_map(|(_, c)| crate::condition_to_query_param(c))
                    .collect();
                ((0..conds.len()).collect(), filters)
            } else {
                (consumed, Vec::new())
            };

        let url = format!(
            "{}{}",
            endpoint,
            self.build_query_string(pagination, &conds, &query_consumed)
        );

        let mut request = self.client.get(&url);
        if let Some(ref auth) = self.auth_header {
            request = request.header("Authorization", auth);
        }

        let response = request
            .send()
            .await
            .map_err(|e| error!("API request failed", url = url, detail = e))?;

        if !response.status().is_success() {
            return Err(error!(
                "API returned error status",
                url = url,
                status = response.status().as_u16()
            ));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| error!("Failed to parse API response as JSON", detail = e))?;

        let data = self.extract_array(&body, table_name)?;

        let mut records = IndexMap::new();
        for (row_idx, item) in data.iter().enumerate() {
            let obj = item
                .as_object()
                .ok_or_else(|| error!("API data item is not an object", index = row_idx))?;

            // Extract ID from the configured id_field, or use row index
            let id = id_field
                .and_then(|field| obj.get(field))
                .and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| row_idx.to_string());

            // The HTTP body parses as JSON for free; convert to CBOR
            // at this single boundary so the rest of the pipeline
            // (Table, Vista) sees the universal carrier.
            let mut record: Record<CborValue> = Record::new();
            for (k, v) in obj {
                let cbor = CborValue::serialized(v).map_err(|e| {
                    error!(
                        "JSON → CBOR conversion failed",
                        field = k.clone(),
                        detail = e.to_string()
                    )
                })?;
                record.insert(k.clone(), cbor);
            }

            records.insert(id, record);
        }

        // Client-side filtering (FilterStrategy::Client): drop rows that
        // don't match the non-path eq-conditions. A condition whose field
        // is absent from a row is treated as a pass (it was a path/request
        // param, not a record field) — mirroring the AWS connector and the
        // Mercury CLI's own post-fetch `_filter_deployments`.
        if !client_filters.is_empty() {
            records.retain(|_id, record| {
                client_filters
                    .iter()
                    .all(|(field, want)| match record.get(field) {
                        Some(v) => crate::cbor_to_query_string(v).as_deref() == Some(want.as_str()),
                        None => true,
                    })
            });
        }

        Ok(records)
    }
}

fn urlencode(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// Walk an `Expression`'s parameter tree and force any `Deferred`
/// branches to their resolved form. Used at the `fetch_records`
/// boundary so the URL builder only sees sync scalars.
///
/// Recursion lives on the heap (boxed) because the future's body
/// contains another `async` call of the same shape — Rust can't size
/// a directly-recursive `async fn` without indirection.
fn resolve_deferreds(
    mut expr: Expression<CborValue>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Expression<CborValue>>> + Send>> {
    Box::pin(async move {
        for param in expr.parameters.iter_mut() {
            match param {
                ExpressiveEnum::Deferred(deferred) => {
                    *param = deferred.call().await?;
                }
                ExpressiveEnum::Nested(inner) => {
                    let resolved = resolve_deferreds(inner.clone()).await?;
                    *inner = resolved;
                }
                ExpressiveEnum::Scalar(_) => {}
            }
        }
        Ok(expr)
    })
}

impl RestApi {
    /// Pull the row array out of the response body, according to the
    /// configured `ResponseShape`.
    fn extract_array<'a>(
        &self,
        body: &'a serde_json::Value,
        table_name: &str,
    ) -> Result<&'a Vec<serde_json::Value>> {
        match &self.response_shape {
            ResponseShape::BareArray => body.as_array().ok_or_else(|| {
                error!("Expected response body to be a JSON array (BareArray shape)")
            }),
            ResponseShape::Wrapped { array_key } => body[array_key].as_array().ok_or_else(|| {
                error!(
                    "Response missing array under wrapper key",
                    array_key = array_key
                )
            }),
            ResponseShape::WrappedByTableName => body[table_name].as_array().ok_or_else(|| {
                error!(
                    "Response missing array under table-name key",
                    table_name = table_name
                )
            }),
        }
    }
}

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
    auth_header: Option<String>,
    response_shape: ResponseShape,
    pagination: PaginationParams,
    no_pagination: bool,
    filter_strategy: FilterStrategy,
}

impl RestApiBuilder {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            auth_header: None,
            response_shape: ResponseShape::default(),
            pagination: PaginationParams::default(),
            no_pagination: false,
            filter_strategy: FilterStrategy::default(),
        }
    }

    /// Set the Authorization header value (e.g. "Bearer `<token>`").
    pub fn auth(mut self, auth: impl Into<String>) -> Self {
        self.auth_header = Some(auth.into());
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

    pub fn build(self) -> RestApi {
        RestApi {
            base_url: self.base_url,
            client: reqwest::Client::new(),
            auth_header: self.auth_header,
            response_shape: self.response_shape,
            pagination: self.pagination,
            no_pagination: self.no_pagination,
            filter_strategy: self.filter_strategy,
        }
    }
}
