//! `GET /`: one page of the cached listing, or that page as a watch.

use std::collections::{BTreeMap, BTreeSet};

use axum::Json;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use vantage_dataset::prelude::ReadableValueSet;

use super::ApiResult;
use super::state::ApiState;
use super::wire::{event_line, ndjson_response, project};

#[derive(serde::Deserialize, Default)]
pub(super) struct ListParams {
    offset: Option<usize>,
    limit: Option<usize>,
    watch: Option<bool>,
}

pub(super) async fn listing(
    State(st): State<ApiState>,
    Query(q): Query<ListParams>,
) -> ApiResult<Response> {
    let offset = q.offset.unwrap_or(0);
    let limit = q.limit.unwrap_or(st.page_size);
    if q.watch.unwrap_or(false) {
        return watch_listing(st, offset, limit).await;
    }

    // Plain GET: a window over the cache, no hydration. Instant regardless
    // of how many augment columns are still unfilled.
    let all = st.dio.cached().list_values().await?;
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

/// The watch mode: open a scenery for the requested page and stream row
/// events for as long as the client stays connected. The scenery is owned by
/// the response stream — when the client disconnects the stream drops, the
/// scenery's guard aborts its tasks, and its queued augment work is
/// withdrawn.
async fn watch_listing(st: ApiState, offset: usize, limit: usize) -> ApiResult<Response> {
    let demand: Vec<String> = st.columns.iter().map(|(_, field)| field.clone()).collect();
    let scenery = st
        .dio
        .table_scenery()
        .columns(demand)
        // Size list pages so a fresh scenery's first page already covers the
        // watched window. Saturating: an absurd offset must not panic.
        .page_size(offset.saturating_add(limit).max(1))
        .open()
        .await?;
    scenery.set_viewport(offset..offset.saturating_add(limit), 0..0);
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
