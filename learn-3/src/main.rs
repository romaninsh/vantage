mod category;
mod product;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use category::{Category, CategoryTable};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use vantage_sql::prelude::*;
use vantage_table::pagination::Pagination;
use vantage_types::Entity;

static DB: OnceLock<SqliteDB> = OnceLock::new();

fn db() -> SqliteDB {
    DB.get().expect("database not initialised").clone()
}

struct ApiError {
    status: StatusCode,
    message: String,
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
        let status = if message.contains("no row found") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        eprintln!("API error: {:?}", e);
        Self { status, message }
    }
}

type Params = HashMap<String, String>;
type ApiResult<T> = Result<T, ApiError>;

#[derive(Deserialize, Default)]
struct ListQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    q: Option<String>,
}

fn crud<E, F>(make_table: F) -> Router
where
    F: Fn(SqliteDB, &Params) -> Table<SqliteDB, E> + Send + Sync + 'static,
    E: Entity<AnySqliteType> + Serialize + DeserializeOwned + Send + Sync + 'static,
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
                    let entity = f(db(), &params).get(&id).await?;
                    ApiResult::Ok(Json(entity))
                }
            })
            .patch({
                let f = f.clone();
                move |Path(params): Path<Params>, Json(partial): Json<E>| async move {
                    let id = params["id"].clone();
                    let updated = f(db(), &params).patch(&id, &partial).await?;
                    ApiResult::Ok(Json(updated))
                }
            })
            .delete({
                let f = f;
                move |Path(params): Path<Params>| async move {
                    let id = params["id"].clone();
                    f(db(), &params).delete(&id).await?;
                    ApiResult::Ok(StatusCode::NO_CONTENT)
                }
            }),
        )
}

#[tokio::main]
async fn main() -> VantageResult<()> {
    let conn = SqliteDB::connect("sqlite:products.db")
        .await
        .context("Failed to connect to products.db")?;
    DB.set(conn).ok();

    let app = Router::new()
        .nest("/categories", crud(|db, _| Category::table(db).clone()))
        .nest(
            "/categories/{cat_id}/products",
            crud(|db, p| {
                let cat_id: i64 = p["cat_id"].parse().unwrap();
                let mut c = Category::table(db).clone();
                c.add_condition(c.id().eq(cat_id));
                c.ref_products()
            }),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
