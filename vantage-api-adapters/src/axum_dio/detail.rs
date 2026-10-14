//! `GET /{id}`: one hydrated record, or that record as a watch.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use vantage_dataset::prelude::ReadableValueSet;

use super::ApiResult;
use super::state::ApiState;
use super::wire::{event_line, ndjson_response};

#[derive(serde::Deserialize, Default)]
pub(super) struct WatchParam {
    watch: Option<bool>,
}

pub(super) async fn detail(
    State(st): State<ApiState>,
    Path(id): Path<String>,
    Query(q): Query<WatchParam>,
) -> ApiResult<Response> {
    if q.watch.unwrap_or(false) {
        return watch_detail(st, id).await;
    }
    // Bounded facade read: hydrates this row (through the shared scheduler)
    // before returning, so the response always carries the augment columns.
    let key = st.cache_key(&id);
    let row = st
        .dio
        .cached()
        .get_value(key.clone())
        .await?
        .ok_or_else(|| st.not_found(&key))?;
    Ok(Json(st.record_object(&row)).into_response())
}

async fn watch_detail(st: ApiState, id: String) -> ApiResult<Response> {
    // Hydrate first so the watch opens on a complete record instead of
    // sitting on an unfilled one.
    let key = st.cache_key(&id);
    let row = st
        .dio
        .cached()
        .get_value(key.clone())
        .await?
        .ok_or_else(|| st.not_found(&key))?;
    let scenery = st.dio.record_scenery(key).open().await?;
    let mut generations = scenery.subscribe();
    // Open on the subscribed scenery's snapshot, not the pre-subscription
    // read: a change landing between the two would otherwise never produce
    // a MODIFIED line.
    let initial = scenery
        .record()
        .map(|current| st.record_object(&current.record))
        .unwrap_or_else(|| st.record_object(&row));

    let stream = async_stream::stream! {
        let mut last = initial;
        yield event_line("ADDED", last.clone());
        loop {
            if generations.changed().await.is_err() {
                break;
            }
            let Some(current) = scenery.record() else { continue };
            let object = st.record_object(&current.record);
            if object != last {
                last = object.clone();
                yield event_line("MODIFIED", object);
            }
        }
    };
    Ok(ndjson_response(stream))
}
