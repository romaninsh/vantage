use anyhow::Context;
use bakery_api::app;
use bakery_model::connect_postgres;
use clap::Parser;
use tower_http::trace::TraceLayer;

mod config;
mod products;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt::init();

    let config = config::Config::parse();
    connect_postgres().await?;

    println!("Listening on http://0.0.0.0:3000");

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", config.host, config.port))
        .await
        .unwrap();
    axum::serve(listener, app().layer(TraceLayer::new_for_http()))
        .await
        .with_context(|| "Failed to serve")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt; // for `call`, `oneshot`, and `ready`

    #[tokio::test]
    async fn test_hello() {
        let app = app();
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), hyper::StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"Hello, World!");
    }
}
