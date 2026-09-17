//! Shared by the transport contract tests: an observer that records event
//! tags in order, and wiremock mounts.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use vantage_api_client::{TransportEvent, TransportObserver};
use wiremock::matchers::{header, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
pub struct Recorder {
    tags: Mutex<Vec<String>>,
    keys: Mutex<Vec<String>>,
}

impl TransportObserver for Recorder {
    fn on_event(&self, key: &str, event: TransportEvent) {
        let tag = match event {
            TransportEvent::Started => "started".to_string(),
            TransportEvent::Succeeded { status, .. } => format!("ok:{status}"),
            TransportEvent::Failed { error, .. } => format!("failed:{}", error.kind_name()),
            TransportEvent::Cancelled => "cancelled".to_string(),
            TransportEvent::RetryScheduled { attempt, .. } => format!("retry:{attempt}"),
            TransportEvent::BreakerOpened { .. } => "opened".to_string(),
            TransportEvent::BreakerClosed => "closed".to_string(),
            TransportEvent::RowsPulled { n } => format!("rows:{n}"),
            TransportEvent::WritePushed => "write".to_string(),
        };
        self.tags.lock().unwrap().push(tag);
        self.keys.lock().unwrap().push(key.to_string());
    }
}

impl Recorder {
    /// Event tags in the order they were reported.
    pub fn tags(&self) -> Vec<String> {
        self.tags.lock().unwrap().clone()
    }

    /// The observer key each event was reported under, same order.
    pub fn keys(&self) -> Vec<String> {
        self.keys.lock().unwrap().clone()
    }
}

/// `n` responses with `status` and `body`, highest priority, then whatever
/// else is mounted.
pub async fn mount_n(server: &MockServer, verb: &str, status: u16, body: &str, n: u64) {
    Mock::given(method(verb))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .up_to_n_times(n)
        .with_priority(1)
        .mount(server)
        .await;
}

/// A permanent response.
pub async fn mount(server: &MockServer, verb: &str, status: u16, body: &str) {
    Mock::given(method(verb))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(server)
        .await;
}

/// A permanent response that only matches requests carrying the `name: value`
/// header — a request without it goes unmatched and fails the test at the
/// server rather than at an assertion.
pub async fn mount_with_header(
    server: &MockServer,
    verb: &str,
    (name, value): (&str, &str),
    status: u16,
    body: &str,
) {
    Mock::given(method(verb))
        .and(header(name, value))
        .respond_with(ResponseTemplate::new(status).set_body_string(body))
        .mount(server)
        .await;
}

/// A client that stamps `user-agent: {agent}` on every request it sends.
pub fn client_with_user_agent(agent: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(agent)
        .build()
        .unwrap()
}

pub async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

pub fn recorder() -> Arc<Recorder> {
    Arc::new(Recorder::default())
}
