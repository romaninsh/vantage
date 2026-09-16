//! Glue between this crate and `vantage-api-pool`'s `ResilientClient`:
//! how a datasource's settings become a client, how the `Priority`
//! task-local becomes a call policy, and how a transport error becomes a
//! `VantageError` that keeps the status and the server's message.

use std::sync::Arc;

use vantage_api_pool::resilient::{CallPolicy, ClientError, ResilientClient, TransportObserver};
use vantage_core::{Priority, VantageError, error};

/// What both API builders collect for the shared client.
pub(crate) struct ClientConfig {
    pub max_parallel: usize,
    pub rate_limit: Option<f64>,
    pub observer: Option<(Arc<str>, Arc<dyn TransportObserver>)>,
    pub http: Option<reqwest::Client>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            max_parallel: 4,
            rate_limit: None,
            observer: None,
            http: None,
        }
    }
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientConfig")
            .field("max_parallel", &self.max_parallel)
            .field("rate_limit", &self.rate_limit)
            .field("observer", &self.observer.as_ref().map(|(k, _)| k))
            .field("http", &self.http.is_some())
            .finish()
    }
}

impl Clone for ClientConfig {
    fn clone(&self) -> Self {
        Self {
            max_parallel: self.max_parallel,
            rate_limit: self.rate_limit,
            observer: self.observer.clone(),
            http: self.http.clone(),
        }
    }
}

pub(crate) fn build_client(cfg: ClientConfig) -> ResilientClient {
    let mut b = ResilientClient::builder()
        .max_parallel(cfg.max_parallel)
        .default_breaker();
    if let Some(rate) = cfg.rate_limit {
        b = b.rate_limit(rate);
    }
    if let Some((key, observer)) = cfg.observer {
        b = b.observer(key, observer);
    }
    if let Some(http) = cfg.http {
        b = b.http_client(http);
    }
    b.build()
}

/// Essential waits retry until cancelled and wait for the breaker's probe;
/// background work makes one attempt and fails fast.
pub(crate) fn policy_for(priority: Priority) -> CallPolicy {
    if priority.is_essential() {
        CallPolicy::essential()
    } else {
        CallPolicy::background()
    }
}

/// How much of the server's error body an error message keeps.
const BODY_IN_ERROR: usize = 500;

pub(crate) fn client_error(e: ClientError, what: &'static str, url: &str) -> VantageError {
    let body = e.body.as_deref().map(|b| {
        let end = b
            .char_indices()
            .nth(BODY_IN_ERROR)
            .map_or(b.len(), |(i, _)| i);
        b[..end].to_string()
    });
    match (e.status(), body) {
        (Some(status), Some(body)) => error!(
            what,
            url = url,
            status = status,
            attempts = e.attempts,
            kind = e.kind_name(),
            body = body
        ),
        (Some(status), None) => error!(
            what,
            url = url,
            status = status,
            attempts = e.attempts,
            kind = e.kind_name()
        ),
        (None, _) => error!(
            what,
            url = url,
            attempts = e.attempts,
            kind = e.kind_name(),
            detail = e.to_string()
        ),
    }
}
