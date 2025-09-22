use axum::{response::IntoResponse, routing::get, Json, Router};
use bakery_model::product::Product;
use vantage::prelude::*;

pub fn router_products() -> Router {
    Router::new().route("/", get(list_products))
}

async fn list_products() -> impl IntoResponse {
    // We will work with Product Set
    let products = Product::table();

    //
    let data = products
        .query_for_field_names(&["id", "name"])
        .get_all_untyped()
        .await
        .unwrap();

    Json(data)
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
    async fn list_products() {
        connect_postgres().await.unwrap();
        let app = app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/products")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json_body: Value = serde_json::from_slice(&body).unwrap();

        assert!(json_body.is_array());

        assert_eq!(
            json_body,
            json!([
                {"id": 1, "name": "Flux Capacitor Cupcake"},
                {"id": 2, "name": "DeLorean Doughnut"},
                {"id": 3, "name": "Time Traveler Tart"},
                {"id": 4, "name": "Enchantment Under the Sea Pie"},
                {"id": 5, "name": "Hoverboard Cookies"}
            ])
        );
    }
}
