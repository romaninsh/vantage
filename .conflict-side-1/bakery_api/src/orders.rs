use axum::{response::IntoResponse, routing::get, Json, Router};
use bakery_model::*;
use serde::Deserialize;
use vantage::{prelude::*, sql::query::SqlQuery};

#[derive(Deserialize)]
struct OrderRequest {
    client_id: i32,
}

#[derive(Deserialize)]
struct Pagination {
    #[serde(default)]
    page: i64,
    #[serde(default = "per_page_default")]
    per_page: i64,
}

pub fn per_page_default() -> i64 {
    10
}

pub fn router_orders() -> Router {
    Router::new().route("/", get(list_orders))
}

async fn list_orders(
    client: axum::extract::Query<OrderRequest>,
    pager: axum::extract::Query<Pagination>,
) -> impl IntoResponse {
    let orders = Client::table()
        .with_id(client.client_id.into())
        .ref_orders();

    let mut query = orders.query();

    // Change the query to include pagination
    query.add_limit(Some(pager.per_page));
    if pager.page > 0 {
        query.add_skip(Some(pager.per_page * pager.page));
    }

    // will serialize type of `Order` struct
    Json(query.get().await.unwrap())
}

#[cfg(test)]
mod tests {
    use crate::app;

    use axum::{body::Body, http::Request};
    use bakery_model::connect_postgres;
    use http_body_util::BodyExt;
    use hyper::StatusCode;
    use serde_json::{json, Value};
    use tower::ServiceExt; // for `call`, `oneshot`, and `ready`

    #[tokio::test]
    async fn list_orders() {
        connect_postgres().await.unwrap();
        let app = app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/orders?client_id=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json_body: Value = serde_json::from_slice(&body).unwrap();

        assert!(json_body.is_array());

        assert_eq!(json_body.as_array().unwrap().len(), 1);

        assert_eq!(
            json_body,
            json!([
                {"id": 1, "client_id": 1, "client_name": "Marty McFly", "total": 893},
            ])
        );
    }
}
