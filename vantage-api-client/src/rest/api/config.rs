//! How an API shapes its responses and takes paging, filters and sorts.

/// How the API wraps its row array in the response body.
///
/// Most public APIs use one of these three shapes. The default is
/// `Wrapped { array_key: "data" }`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum ResponseShape {
    /// Body is a bare JSON array of records.
    /// Example: `GET /users` → `[ {…}, {…} ]`. JSONPlaceholder, GitHub, etc.
    BareArray,

    /// Body is a JSON object with the array under a fixed key.
    /// Example: `GET /users` → `{ "data": [ … ] }`.
    Wrapped {
        /// The key holding the row array.
        array_key: String,
    },

    /// Body is a JSON object with the array under a key matching the
    /// table name. Example (DummyJSON):
    /// `GET /products` → `{ "products": [ … ], "total": …, "skip": …, "limit": … }`.
    WrappedByTableName,
}

impl Default for ResponseShape {
    /// `{ "data": [...] }`, the envelope JSON:API and many hand-rolled
    /// services use.
    fn default() -> Self {
        ResponseShape::Wrapped {
            array_key: "data".to_string(),
        }
    }
}

/// What the position parameter of [`PaginationParams`] counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaginationMode {
    /// A 1-based page index (`?_page=3&_limit=10` is rows 20–29). The
    /// JSON Server convention.
    #[default]
    Page,
    /// A 0-based item offset (`?skip=20&limit=10` is rows 20–29).
    /// DummyJSON, Django REST Framework's `offset`, and most SQL-backed
    /// APIs.
    Skip,
}

/// Names of the paging query parameters the API expects, and what the
/// position parameter counts.
///
/// Defaults to `("_page", "_limit")` in [`PaginationMode::Page`] — the
/// JSON Server convention used by JSONPlaceholder. DummyJSON uses
/// `("skip", "limit")` in [`PaginationMode::Skip`]. Set it with
/// [`RestApiBuilder::pagination_params`](super::RestApiBuilder::pagination_params).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PaginationParams {
    /// Name of the position parameter: the page index or the item offset,
    /// per [`mode`](Self::mode).
    pub page: String,
    /// Name of the page-size parameter.
    pub limit: String,
    /// What the position parameter counts.
    pub mode: PaginationMode,
}

impl PaginationParams {
    /// A 1-based page index under `page` and the page size under `limit`.
    pub fn page_limit(page: impl Into<String>, limit: impl Into<String>) -> Self {
        Self {
            page: page.into(),
            limit: limit.into(),
            mode: PaginationMode::Page,
        }
    }

    /// A 0-based item offset under `skip` and the page size under `limit`.
    pub fn skip_limit(skip: impl Into<String>, limit: impl Into<String>) -> Self {
        Self {
            page: skip.into(),
            limit: limit.into(),
            mode: PaginationMode::Skip,
        }
    }
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self::page_limit("_page", "_limit")
    }
}

/// How a table's conditions are applied to a request.
///
/// URL `{placeholder}` path segments are always filled from matching
/// eq-conditions regardless of strategy; this governs what happens to
/// the *remaining* (non-path) eq-conditions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterStrategy {
    /// Append remaining eq-conditions as `?field=value` query params
    /// (JSON-Server semantics). The default.
    #[default]
    Query,
    /// Apply remaining eq-conditions as in-memory row filters after the
    /// fetch, never as query params. For APIs whose only server-side
    /// filters are path segments and that reject (or ignore) unknown
    /// query params.
    Client,
}

/// How the API takes a sort: one query param naming the column, with a
/// prefix that flips it to descending (`?ordering=-net`, the Django REST
/// Framework and Launch Library convention). Configuring it makes the REST
/// vista orderable, so a paged grid asks the server for its sort instead of
/// re-sorting the rows it happens to hold.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct OrderingParams {
    /// Name of the sort query parameter (`ordering`).
    pub param: String,
    /// Prefix on the column name that makes the sort descending (`-`).
    pub desc_prefix: String,
}

impl OrderingParams {
    /// Sort by `?{param}=<column>`, descending as `?{param}={desc_prefix}<column>`.
    pub fn new(param: impl Into<String>, desc_prefix: impl Into<String>) -> Self {
        Self {
            param: param.into(),
            desc_prefix: desc_prefix.into(),
        }
    }

    /// The value for `param`: the column, prefixed when descending.
    pub(crate) fn value(&self, field: &str, dir: vantage_vista::SortDirection) -> String {
        match dir {
            vantage_vista::SortDirection::Ascending => field.to_string(),
            vantage_vista::SortDirection::Descending => format!("{}{field}", self.desc_prefix),
        }
    }
}

/// A sort pushed down to the API: column and direction.
pub(crate) type Order<'a> = Option<(&'a str, vantage_vista::SortDirection)>;
