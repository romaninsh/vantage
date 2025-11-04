use axum::http::StatusCode;
use axum::{routing::*, Json, Router};
use serde::{Deserialize, Serialize};

pub mod orders;
pub mod products;

async fn root() -> &'static str {
    "Hello, World!"
}
pub fn app() -> Router {
    Router::new()
        .route("/", get(root))
        .route("/users", post(create_user))
        .nest("/products", products::router_products())
        .nest("/orders", orders::router_orders())
}

async fn create_user(
    // this argument tells axum to parse the request body
    // as JSON into a `CreateUser` type
    Json(payload): Json<CreateUser>,
) -> (StatusCode, Json<User>) {
    // insert your application logic here
    let user = User {
        id: 1337,
        username: payload.username,
    };

    // this will be converted into a JSON response
    // with a status code of `201 Created`
    (StatusCode::CREATED, Json(user))
}
// the input to our `create_user` handler
#[derive(Deserialize)]
struct CreateUser {
    username: String,
}

// the output to our `create_user` handler
#[derive(Serialize)]
struct User {
    id: u64,
    username: String,
}
