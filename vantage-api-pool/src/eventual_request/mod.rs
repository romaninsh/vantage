use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use reqwest::{Client, Request, Response};
use tokio::{
    sync::{oneshot, Mutex},
    time::sleep,
};
use tracing::warn;

/// Eventual request will be sent to the client... eventually. Might take
/// a little bit of time, but we will get that response back.
pub struct EventualRequest<T: Sync + Send + Sized> {
    id: Option<usize>,

    request: Option<Request>,
    response: Option<Response>,

    queued: Option<Instant>,    // time when request got queued up
    retries: usize,             // number of times we retried the request
    requested: Option<Instant>, // time when request was sent last time
    responsed: Option<Instant>, // time when response was received. responsed-requested = latency

    in_queue: Duration,
    latency: Duration,

    pub metadata: Option<T>, // additional user metadata
}

impl<T: Sync + Send + Sized> EventualRequest<T> {
    pub fn new(request: impl Into<Request>, metadata: Option<T>) -> Self {
        EventualRequest {
            id: None,
            request: Some(request.into()),
            response: None,
            queued: None,
            retries: 0,
            requested: None,
            responsed: None,

            in_queue: Duration::ZERO,
            latency: Duration::ZERO,

            metadata,
        }
    }

    pub async fn register(
        &mut self,
        id: usize,
        ch: Arc<Mutex<HashMap<usize, oneshot::Sender<EventualRequest<T>>>>>,
    ) -> oneshot::Receiver<EventualRequest<T>> {
        self.id = Some(id);
        let (sender, receiver) = oneshot::channel();

        ch.lock().await.insert(id, sender);
        receiver
    }

    pub fn get_id(&self) -> Option<usize> {
        self.id
    }

    pub fn request(&self) -> Option<&Request> {
        self.request.as_ref()
    }

    pub fn time_queue_start(&mut self) {
        if self.queued.is_some() {
            self.time_queue_stop();
        }
        self.queued = Some(Instant::now())
    }

    pub fn time_queue_stop(&mut self) {
        self.in_queue += Instant::now().duration_since(self.queued.unwrap());
        self.queued = None;
    }

    pub fn time_request_start(&mut self) {
        if self.requested.is_some() {
            self.time_request_stop();
        }
        self.requested = Some(Instant::now())
    }

    pub fn time_request_stop(&mut self) {
        self.responsed = Some(Instant::now());
        self.latency += self
            .responsed
            .unwrap()
            .duration_since(self.requested.unwrap());
    }

    fn extract_retry_delay(&self, response: &Response) -> Option<Duration> {
        if let Some(retry_after) = response.headers().get("retry-after") {
            if let Ok(retry_str) = retry_after.to_str() {
                if let Ok(retry_secs) = retry_str.parse::<u64>() {
                    if retry_secs >= 1 {
                        return Some(Duration::from_secs(retry_secs));
                    }
                }
            }
        }
        None
    }

    fn calculate_backoff_delay(&self) -> Duration {
        // Start at 50ms, power of 1.2, max 10 seconds
        let delay_ms = (50.0 * 1.2_f64.powi(self.retries as i32)).min(10000.0) as u64;
        Duration::from_millis(delay_ms)
    }

    pub async fn execute(&mut self, client: &Client) -> EventualRequestResult {
        // take potentially un-clonable request out of self
        let Some(request) = self.request.take() else {
            return EventualRequestResult::Error("Missing Request".to_string());
        };
        // try to put it back
        self.request = request.try_clone();

        match client.execute(request).await {
            Ok(response) if response.status() == 429 => {
                self.retries += 1;

                let delay = self
                    .extract_retry_delay(&response)
                    .unwrap_or_else(|| self.calculate_backoff_delay());

                self.response = Some(response);
                warn!(attempt = self.retries, ?delay, "received 429, retrying");
                sleep(delay).await;
                EventualRequestResult::Retry
            }
            Ok(response) if response.status().is_server_error() => {
                self.retries += 1;
                let status = response.status();

                let delay = self.calculate_backoff_delay();

                self.response = Some(response);
                sleep(delay).await;
                warn!(%status, attempt = self.retries, ?delay, "received 5xx, retrying");
                EventualRequestResult::Retry
            }
            Ok(response) if response.status().is_success() => {
                self.response = Some(response);
                EventualRequestResult::Success
            }
            Ok(response) => {
                let status = response.status();
                let error = format!("Status {} returned", status);
                self.response = Some(response);
                EventualRequestResult::Error(error)
            }
            Err(err) => {
                self.retries += 1;

                let delay = self.calculate_backoff_delay();

                sleep(delay).await;
                warn!(error = %err, attempt = self.retries, ?delay, "network error, retrying");
                EventualRequestResult::Retry
            }
        }
    }

    pub fn response(&mut self) -> Option<Response> {
        self.response.take()
    }
}

pub enum EventualRequestResult {
    Success,
    Retry,
    Error(String),
}
