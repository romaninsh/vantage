//! The state every handler shares: the Dio and the router's configuration.

use std::sync::Arc;

use axum::http::StatusCode;
use ciborium::Value as CborValue;
use vantage_diorama::Dio;
use vantage_types::Record;

use super::wire::record_json;
use super::{ApiError, IdMap, RecordProjection, not_found};

#[derive(Clone)]
pub(super) struct ApiState {
    pub(super) dio: Dio,
    pub(super) columns: Arc<[(String, String)]>,
    pub(super) page_size: usize,
    pub(super) key_field: Option<Arc<str>>,
    pub(super) id_map: Option<Arc<IdMap>>,
    pub(super) record_projection: Option<Arc<RecordProjection>>,
    pub(super) not_found_message: Option<Arc<str>>,
}

impl ApiState {
    pub(super) fn cache_key(&self, path_id: &str) -> String {
        match &self.id_map {
            Some(map) => map(path_id),
            None => path_id.to_string(),
        }
    }

    pub(super) fn record_object(&self, record: &Record<CborValue>) -> serde_json::Value {
        match &self.record_projection {
            Some(projection) => projection(record),
            None => record_json(record),
        }
    }

    pub(super) fn not_found(&self, cache_key: &str) -> ApiError {
        match &self.not_found_message {
            Some(message) => ApiError {
                status: StatusCode::NOT_FOUND,
                message: message.to_string(),
            },
            None => not_found(cache_key),
        }
    }
}
