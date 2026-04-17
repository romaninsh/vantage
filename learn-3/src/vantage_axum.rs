//! Generic CRUD plumbing over any vantage `Table<MongoDB, E>`.
//!
//! One `crud(make_table)` call mounts a full REST surface — `GET/POST` on the
//! list path, `GET/PATCH/DELETE` on `/{id}` — driven entirely by what the
//! closure returns. Error rendering (`ApiError`) and query-string parsing
//! (`ListQuery`) live here too so `main.rs` stays about *routes*, not HTTP
//! mechanics.

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use vantage_mongodb::prelude::*;

use crate::db::db;

pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

impl From<VantageError> for ApiError {
    fn from(e: VantageError) -> Self {
        let message = e.to_string();
        let status = if message.contains("no row found") || message.contains("Document not found") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        eprintln!("API error: {:?}", e);
        Self { status, message }
    }
}

pub type Params = HashMap<String, String>;
pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Deserialize, Default)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub q: Option<String>,
}

pub fn crud<E, F>(make_table: F) -> Router
where
    F: Fn(MongoDB, &Params) -> Table<MongoDB, E> + Send + Sync + 'static,
    E: Entity<AnyMongoType> + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    let f = Arc::new(make_table);
    Router::new()
        .route(
            "/",
            get({
                let f = f.clone();
                move |p: Option<Path<Params>>, Query(q): Query<ListQuery>| async move {
                    let params = p.map(|Path(p)| p).unwrap_or_default();
                    let mut t = f(db(), &params);
                    if q.page.is_some() || q.per_page.is_some() {
                        t.set_pagination(Some(Pagination::new(
                            q.page.unwrap_or(1),
                            q.per_page.unwrap_or(50),
                        )));
                    }
                    if let Some(term) = q.q.as_deref() {
                        t.add_search(term);
                    }
                    let rows = t.list().await?;
                    ApiResult::Ok(Json::<Vec<E>>(rows.into_values().collect()))
                }
            })
            .post({
                let f = f.clone();
                move |p: Option<Path<Params>>, Json(entity): Json<E>| async move {
                    let params = p.map(|Path(p)| p).unwrap_or_default();
                    let id = f(db(), &params).insert_return_id(&entity).await?;
                    ApiResult::Ok(Json(serde_json::json!({ "id": id })))
                }
            }),
        )
        .route(
            "/{id}",
            get({
                let f = f.clone();
                move |Path(params): Path<Params>| async move {
                    let id = params["id"].clone();
                    let entity = f(db(), &params).get(id).await?;
                    ApiResult::Ok(Json(entity))
                }
            })
            .patch({
                let f = f.clone();
                move |Path(params): Path<Params>, Json(partial): Json<E>| async move {
                    let id: MongoId = params["id"].clone().into();
                    let updated = f(db(), &params).patch(&id, &partial).await?;
                    ApiResult::Ok(Json(updated))
                }
            })
            .delete({
                let f = f;
                move |Path(params): Path<Params>| async move {
                    let id: MongoId = params["id"].clone().into();
                    f(db(), &params).delete(&id).await?;
                    ApiResult::Ok(StatusCode::NO_CONTENT)
                }
            }),
        )
}
