//! Axum router over a Dio — a kubernetes-style API surface for a cached,
//! augmented table.
//!
//! Two endpoints, each in two modes:
//!
//! - `GET /?offset=&limit=` — one page of the cached listing, instantly.
//!   Deliberately does NOT hydrate: a plain GET is not a standing view, so it
//!   serves the Dio's current knowledge (augmented values appear once some
//!   view has paid for them).
//! - `GET /?offset=&limit=&watch=true` — the same page as a **watch**: the
//!   connection stays open and streams NDJSON events, kubernetes-style —
//!   `{"type":"ADDED","object":{…}}` per row, then `MODIFIED` lines as rows
//!   change. A watch is a real [`TableScenery`](vantage_diorama::TableScenery):
//!   it declares the configured
//!   columns as its demand and its page as the viewport, which is exactly
//!   what drives augmentation. Closing the connection drops the scenery —
//!   its queued detail fetches are withdrawn and its demand drains.
//! - `GET /{id}` — one record with every column, hydrated: a bounded facade
//!   read that blocks until the row's augment columns are filled (cached, so
//!   the cost is paid once). Fetches share the augment scheduler, so a
//!   detail GET racing a watch never downloads a row twice.
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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use ciborium::Value as CborValue;
use futures_util::StreamExt;
use serde_json::json;
use vantage_core::VantageError;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::Dio;
use vantage_types::Record;

// ---- Errors — the learn-3 vantage_axum shape -------------------------------

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

// ---- Router builder ---------------------------------------------------------

/// Builder for a kubernetes-style GET + watch router over one [`Dio`].
pub struct DioRouter {
    dio: Dio,
    columns: Vec<(String, String)>,
    page_size: usize,
    key_field: Option<String>,
}

impl DioRouter {
    pub fn new(dio: Dio) -> Self {
        Self {
            dio,
            columns: Vec::new(),
            page_size: 50,
            key_field: None,
        }
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
        };
        Router::new()
            .route("/", get(listing))
            .route("/{id}", get(detail))
            .with_state(state)
    }
}

#[derive(Clone)]
struct ApiState {
    dio: Dio,
    columns: Arc<[(String, String)]>,
    page_size: usize,
    key_field: Option<Arc<str>>,
}

#[derive(serde::Deserialize, Default)]
struct ListParams {
    offset: Option<usize>,
    limit: Option<usize>,
    watch: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
struct WatchParam {
    watch: Option<bool>,
}

// ---- Handlers ---------------------------------------------------------------

async fn listing(State(st): State<ApiState>, Query(q): Query<ListParams>) -> ApiResult<Response> {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(st.page_size);
    if q.watch.unwrap_or(false) {
        return watch_listing(st, offset, limit).await;
    }

    // Plain GET: a window over the cache, no hydration. Instant regardless
    // of how many augment columns are still unfilled.
    let all = st.dio.cache().list_values().await?;
    let total = all.len();
    let items: Vec<serde_json::Value> = all
        .iter()
        .skip(offset)
        .take(limit)
        .enumerate()
        .map(|(i, (_, rec))| project(offset + i, rec, &st.columns))
        .collect();
    Ok(Json(json!({
        "total": total,
        "offset": offset,
        "limit": limit,
        "items": items,
    }))
    .into_response())
}

/// The watch mode: open a scenery for the requested page and stream row
/// events for as long as the client stays connected. The scenery is owned by
/// the response stream — when the client disconnects the stream drops, the
/// scenery's guard aborts its tasks, and its queued augment work is
/// withdrawn.
/// A stable string key for an identity-watch diff.
///
/// Strings key by their raw value (the common case: a text id). Other JSON —
/// numeric ids, Mongo `ObjectId` objects, SurrealDB `Thing` tags — keys by its
/// canonical serialization, so `key_by` works for any backend whose id isn't a
/// bare string. `null`/absent yields no key (the row is skipped from the diff).
fn stable_key(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => serde_json::to_string(other).ok(),
    }
}

async fn watch_listing(st: ApiState, offset: usize, limit: usize) -> ApiResult<Response> {
    let demand: Vec<String> = st.columns.iter().map(|(_, field)| field.clone()).collect();
    let scenery = st
        .dio
        .table_scenery()
        .columns(demand)
        // Every connection is its own standing view: two watchers of
        // different pages must each keep their own viewport (and their own
        // place in the augment scheduler's round-robin) — a shared scenery
        // would hydrate only the last-set window.
        .exclusive()
        // Size list pages so a fresh scenery's first page already covers the
        // watched window. Saturating: an absurd offset must not panic.
        .page_size(offset.saturating_add(limit).max(1))
        .open()
        .await?;
    scenery.set_viewport(offset..offset.saturating_add(limit));
    let mut generations = scenery.subscribe();
    let columns = st.columns.clone();
    let key_field = st.key_field.clone();

    let stream = async_stream::stream! {
        // Diff base. Positional watches key it by row index; identity watches
        // (a `key_by` field is set) key it by that field's value, so a removed
        // row can be reported as `DELETED` and a shifted row isn't re-sent.
        let mut last: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        loop {
            // The index pages lazily and is shared per-query across
            // sceneries — an earlier watch may have built it shallower than
            // this window. Keep asking for list pages until it reaches us;
            // each landed page bumps the generation, which re-runs this
            // check.
            if scenery.has_more() && scenery.row_count() < offset.saturating_add(limit) {
                scenery.request_load_more();
            }
            let end = offset.saturating_add(limit).min(scenery.row_count());
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for idx in offset..end {
                let Some(row) = scenery.row(idx) else { continue };
                let object = project(idx, &row.record, &columns);
                // Diff key: the identity field's value, or the row index.
                let key = match &key_field {
                    Some(field) => match object.get(field.as_ref()).and_then(stable_key) {
                        Some(id) => id,
                        None => continue,
                    },
                    None => idx.to_string(),
                };
                seen.insert(key.clone());
                let kind = match last.get(&key) {
                    Some(previous) if *previous == object => None,
                    Some(_) => Some("MODIFIED"),
                    None => Some("ADDED"),
                };
                if let Some(kind) = kind {
                    last.insert(key, object.clone());
                    yield event_line(kind, object);
                }
            }
            // Identity watches report rows that left the set; positional
            // watches never do — a shrunk list simply stops emitting the tail.
            if key_field.is_some() {
                let gone: Vec<String> =
                    last.keys().filter(|k| !seen.contains(*k)).cloned().collect();
                for key in gone {
                    if let Some(object) = last.remove(&key) {
                        yield event_line("DELETED", object);
                    }
                }
            }
            // Wait for the next generation; the sender lives as long as the
            // scenery, which this stream owns — an error means the Dio died.
            if generations.changed().await.is_err() {
                break;
            }
        }
    };
    Ok(ndjson_response(stream))
}

async fn detail(
    State(st): State<ApiState>,
    Path(id): Path<String>,
    Query(q): Query<WatchParam>,
) -> ApiResult<Response> {
    if q.watch.unwrap_or(false) {
        return watch_detail(st, id).await;
    }
    // Bounded facade read: hydrates this row (through the shared scheduler)
    // before returning, so the response always carries the augment columns.
    let row = st
        .dio
        .vista()
        .get_value(id.clone())
        .await?
        .ok_or_else(|| not_found(&id))?;
    Ok(Json(record_json(&row)).into_response())
}

async fn watch_detail(st: ApiState, id: String) -> ApiResult<Response> {
    // Hydrate first so the watch opens on a complete record instead of
    // sitting on an unfilled one.
    let row = st
        .dio
        .vista()
        .get_value(id.clone())
        .await?
        .ok_or_else(|| not_found(&id))?;
    let scenery = st.dio.record_scenery(id).await?;
    let mut generations = scenery.subscribe();
    // Open on the subscribed scenery's snapshot, not the pre-subscription
    // read: a change landing between the two would otherwise never produce
    // a MODIFIED line.
    let initial = scenery
        .record()
        .map(|current| record_json(&current.record))
        .unwrap_or_else(|| record_json(&row));

    let stream = async_stream::stream! {
        let mut last = initial;
        yield event_line("ADDED", last.clone());
        loop {
            if generations.changed().await.is_err() {
                break;
            }
            let Some(current) = scenery.record() else { continue };
            let object = record_json(&current.record);
            if object != last {
                last = object.clone();
                yield event_line("MODIFIED", object);
            }
        }
    };
    Ok(ndjson_response(stream))
}

// ---- Wire helpers -----------------------------------------------------------

/// Project a record onto the configured columns: `{"index": …, "<name>": …}`.
fn project(
    index: usize,
    record: &Record<CborValue>,
    columns: &[(String, String)],
) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert("index".into(), json!(index));
    for (name, field) in columns {
        let value = record
            .get(field)
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null);
        object.insert(name.clone(), value);
    }
    serde_json::Value::Object(object)
}

/// The whole record as a JSON object — the detail endpoint's shape.
fn record_json(record: &Record<CborValue>) -> serde_json::Value {
    serde_json::to_value(record.as_inner()).unwrap_or(serde_json::Value::Null)
}

/// One kubernetes-style watch line: `{"type":"…","object":{…}}\n`.
fn event_line(kind: &str, object: serde_json::Value) -> String {
    format!("{}\n", json!({ "type": kind, "object": object }))
}

fn ndjson_response(stream: impl futures_util::Stream<Item = String> + Send + 'static) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(
            stream.map(Ok::<_, std::convert::Infallible>),
        ))
        .expect("static parts are valid")
}
