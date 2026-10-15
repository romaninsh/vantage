//! Axum router over a Dio — a kubernetes-style API surface for a cached,
//! augmented table.
//!
//! Two endpoints, each in two modes:
//!
//! - `GET /?offset=&limit=` — one page of the cached listing, instantly.
//!   Does not hydrate: a plain GET is not a standing view, so augmented
//!   values appear once some view has paid for them.
//! - `GET /?offset=&limit=&watch=true` — the same page as a **watch**: the
//!   connection stays open and streams NDJSON events,
//!   `{"type":"ADDED","object":{…}}` per row, then `MODIFIED` lines as rows
//!   change. A watch is a [`TableScenery`](vantage_diorama::TableScenery)
//!   whose demand is the configured columns, which is what drives
//!   augmentation. Closing the connection drops the scenery and withdraws
//!   its queued detail fetches.
//! - `GET /{id}` — one record with every column, hydrated: blocks until the
//!   row's augment columns are filled. Fetches share the augment scheduler,
//!   so a detail GET racing a watch never downloads a row twice.
//! - `GET /{id}?watch=true` — the record as a watch: `ADDED` with the
//!   current value, `MODIFIED` on every change, via a
//!   [`RecordScenery`](vantage_diorama::RecordScenery).
//!
//! ```ignore
//! let api = DioRouter::new(dio)
//!     .with_column("filename", "Key")   // JSON key ← record field
//!     .with_column("size", "Size")
//!     .with_column("rows", "rows")      // augmented — naming it here is what
//!     .with_page_size(50)               // makes watches demand hydration
//!     .into_router();
//! let app = axum::Router::new().nest("/api/files", api);
//! ```

mod detail;
mod listing;
mod state;
mod wire;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use ciborium::Value as CborValue;
use serde_json::json;
use vantage_core::VantageError;
use vantage_diorama::Dio;
use vantage_types::Record;

use state::ApiState;

pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<VantageError> for ApiError {
    fn from(e: VantageError) -> Self {
        tracing::error!(error = ?e, "API error");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: e.to_string(),
        }
    }
}

fn not_found(id: &str) -> ApiError {
    ApiError {
        status: StatusCode::NOT_FOUND,
        message: format!("not found: {id}"),
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Maps the id from the URL path to the Dio cache key.
pub type IdMap = dyn Fn(&str) -> String + Send + Sync;
/// Shapes a record into the JSON object served by the detail endpoint and
/// carried in record-watch events.
pub type RecordProjection = dyn Fn(&Record<CborValue>) -> serde_json::Value + Send + Sync;

/// Builder for a kubernetes-style GET + watch router over one [`Dio`].
pub struct DioRouter {
    dio: Dio,
    columns: Vec<(String, String)>,
    page_size: usize,
    key_field: Option<String>,
    id_map: Option<Arc<IdMap>>,
    record_projection: Option<Arc<RecordProjection>>,
    not_found_message: Option<String>,
}

impl DioRouter {
    pub fn new(dio: Dio) -> Self {
        Self {
            dio,
            columns: Vec::new(),
            page_size: 50,
            key_field: None,
            id_map: None,
            record_projection: None,
            not_found_message: None,
        }
    }

    /// Map the `{id}` path segment to the Dio cache key before any lookup.
    /// Use this when URLs carry a short form of the id — for example a
    /// SurrealDB router that accepts `75KE-F3HG` while the cache keys rows
    /// as `tag:75KE-F3HG`:
    ///
    /// ```ignore
    /// .with_id_map(|id| format!("tag:{id}"))
    /// ```
    pub fn with_id_map(mut self, map: impl Fn(&str) -> String + Send + Sync + 'static) -> Self {
        self.id_map = Some(Arc::new(map));
        self
    }

    /// Shape the detail response and every record-watch `object` with a
    /// closure instead of serving the whole record. Use this to serve an
    /// exact API contract: pick fields, rename them, nest them, and keep
    /// internal columns off the wire.
    pub fn with_record_projection(
        mut self,
        projection: impl Fn(&Record<CborValue>) -> serde_json::Value + Send + Sync + 'static,
    ) -> Self {
        self.record_projection = Some(Arc::new(projection));
        self
    }

    /// The message served with a detail 404. Without it, the response
    /// carries `not found: <cache key>` — set this when clients show the
    /// error text to end users.
    pub fn with_not_found_message(mut self, message: impl Into<String>) -> Self {
        self.not_found_message = Some(message.into());
        self
    }

    /// Expose record field `field` as JSON key `name` in listing rows. The
    /// configured fields double as the watch sceneries' **demand** — naming
    /// an augmented column here is what makes watch connections drive its
    /// hydration.
    pub fn with_column(mut self, name: impl Into<String>, field: impl Into<String>) -> Self {
        self.columns.push((name.into(), field.into()));
        self
    }

    /// Default page size when the request carries no `limit`. Default 50.
    pub fn with_page_size(mut self, n: usize) -> Self {
        self.page_size = n;
        self
    }

    /// Diff the listing watch by the value of record field `field` (a stable
    /// row id) instead of by position. With a key set, the watch is
    /// identity-keyed: a row that leaves the set produces a `DELETED` event, a
    /// row that merely shifts position is not re-sent, and `ADDED`/`MODIFIED`
    /// track the row rather than the slot. Without it, the watch keeps its
    /// positional behaviour (index-diffed, `ADDED`/`MODIFIED` only). The field
    /// must be exposed via [`with_column`](Self::with_column).
    pub fn key_by(mut self, field: impl Into<String>) -> Self {
        self.key_field = Some(field.into());
        self
    }

    /// Build the router: `GET /` and `GET /{id}`, both honouring
    /// `?watch=true`. Nest it wherever the resource should live.
    pub fn into_router(self) -> Router {
        let state = ApiState {
            dio: self.dio,
            columns: Arc::from(self.columns),
            page_size: self.page_size,
            key_field: self.key_field.map(Arc::from),
            id_map: self.id_map,
            record_projection: self.record_projection,
            not_found_message: self.not_found_message.map(Arc::from),
        };
        Router::new()
            .route("/", get(listing::listing))
            .route("/{id}", get(detail::detail))
            .with_state(state)
    }
}
