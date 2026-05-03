use std::{
    collections::HashMap,
    sync::{atomic::AtomicUsize, Arc},
};

use reqwest::{Request, Response};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{warn, Instrument as _};

use crate::EventualRequest;

pub struct EventualRequestMatcher<T: Send + Sync + Sized> {
    thread_handle: tokio::task::JoinHandle<()>,
    request_sender: mpsc::Sender<EventualRequest<T>>,
    pending_requests: Arc<Mutex<HashMap<usize, oneshot::Sender<EventualRequest<T>>>>>,
    seq_id: AtomicUsize,
}

impl<T: Send + Sync + Sized + 'static> EventualRequestMatcher<T> {
    pub fn new(
        request_sender: mpsc::Sender<EventualRequest<T>>,
        response_receiver: mpsc::Receiver<EventualRequest<T>>,
    ) -> Self {
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let thread_handle = tokio::spawn(
            Self::worker_thread(pending_requests.clone(), response_receiver).in_current_span(),
        );

        Self {
            request_sender,
            thread_handle,
            pending_requests,
            seq_id: AtomicUsize::new(0),
        }
    }

    pub fn seq_id(&self) -> usize {
        self.seq_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn worker_thread(
        ch: Arc<Mutex<HashMap<usize, oneshot::Sender<EventualRequest<T>>>>>,
        mut response_receiver: mpsc::Receiver<EventualRequest<T>>,
    ) {
        loop {
            let Some(response) = response_receiver.recv().await else {
                warn!("response channel closed, matcher shutting down");
                break;
            };

            match ch.lock().await.remove(&response.get_id().unwrap()) {
                Some(sender) => sender.send(response).unwrap_or_else(|response| {
                    warn!(id = ?response.get_id(), "failed to send response (caller dropped receiver)");
                }),
                None => warn!(id = ?response.get_id(), "no pending request found for response"),
            }
        }
    }

    pub async fn send(
        &self,
        request: impl Into<Request>,
        metadata: Option<T>,
    ) -> Result<Response, String> {
        let mut request = EventualRequest::new(request, metadata);
        let receiver = request
            .register(self.seq_id(), self.pending_requests.clone())
            .await;

        self.request_sender
            .send(request)
            .await
            .map_err(|e| e.to_string())?;

        let mut result = receiver.await.map_err(|e| e.to_string())?;

        result.response().ok_or("No response".to_string())
    }

    pub async fn shutdown(self) -> Result<(), tokio::task::JoinError> {
        // Close channel, this will cause worker to finish
        drop(self.request_sender);

        // wait for worker to finish
        self.thread_handle.await
    }
}
