//! Web-framework adapters for Vantage's Dio/Scenery layer — the server-side
//! counterpart of `dataset-ui-adapters`. A UI adapter binds a scenery to a
//! widget; an API adapter binds it to an HTTP surface, so a browser (or any
//! HTTP client) becomes just another scenery consumer.
//!
//! One module per framework, each behind a feature of the same name:
//!
//! - `axum` — [`axum_dio::DioRouter`], a kubernetes-style GET + watch
//!   router over a Dio, and [`axum_action::ActionRouter`], which mounts
//!   a model's [`ModelAction`](vantage_action::ModelAction) set as
//!   POST routes.

#[cfg(feature = "axum")]
pub mod axum_action;
#[cfg(feature = "axum")]
pub mod axum_dio;
