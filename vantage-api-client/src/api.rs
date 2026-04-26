use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
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
#[derive(Clone, Debug)]
pub struct RestApi {
    base_url: String,
    client: reqwest::Client,
    pub(crate) auth_header: Option<String>,
    response_shape: ResponseShape,
    pagination: PaginationParams,
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

    /// Build the endpoint URL for a given table name. No query string.
    fn endpoint_url(&self, table_name: &str) -> String {
        format!("{}/{}", self.base_url, table_name)
    }

    /// Build the combined query-string from pagination + conditions.
    /// Conditions that don't peel cleanly into eq pairs are skipped (we
    /// could fail loudly here, but silently ignoring matches the v1
    /// "best effort" stance — the caller still gets correct data, just
    /// with less efficient filtering).
    fn build_query_string<'a>(
        &self,
        pagination: Option<&Pagination>,
        conditions: impl IntoIterator<Item = &'a Expression<Value>>,
    ) -> String {
        let mut params: Vec<(String, String)> = Vec::new();

        // Pagination first — matches the order users see in the URL bar.
        if let Some(p) = pagination {
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
        for cond in conditions {
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
        conditions: impl IntoIterator<Item = &'a Expression<Value>>,
    ) -> Result<IndexMap<String, Record<serde_json::Value>>> {
        let url = format!(
            "{}{}",
            self.endpoint_url(table_name),
            self.build_query_string(pagination, conditions)
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

            let record: Record<serde_json::Value> =
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

            records.insert(id, record);
        }

        Ok(records)
    }
}

fn urlencode(s: &str) -> String {
    urlencoding::encode(s).into_owned()
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
}

impl RestApiBuilder {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            auth_header: None,
            response_shape: ResponseShape::default(),
            pagination: PaginationParams::default(),
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

    pub fn build(self) -> RestApi {
        RestApi {
            base_url: self.base_url,
            client: reqwest::Client::new(),
            auth_header: self.auth_header,
            response_shape: self.response_shape,
            pagination: self.pagination,
        }
    }
}
