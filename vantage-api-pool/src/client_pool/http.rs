use std::sync::Arc;

use rust_decimal::prelude::*;
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{eventual_request::EventualRequestResult, EventualRequest, KeyedRateLimiter};

pub struct HttpClientPool<T: Sync + Send + Sized + 'static> {
    workers: usize,
    rate_limit: Option<Arc<KeyedRateLimiter<usize>>>, // on/off is immutable, but rate itself can be adjusted
    _use_dampener: bool,                              // immutable
    shared_handles: Vec<JoinHandle<()>>,              // immutable
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Sync + Send + Sized> HttpClientPool<T> {
    pub fn new(
        workers: usize,
        rate_limit: Option<Decimal>,
        use_dampener: bool,
        request_receiver: mpsc::Receiver<EventualRequest<T>>,
    ) -> (mpsc::Receiver<EventualRequest<T>>, Self) {
        let request_receiver = Arc::new(tokio::sync::Mutex::new(request_receiver));
        let (response_sender, response_receiver) = mpsc::channel(100);

        let mut shared_handles = Vec::new();

        let rate_limit = rate_limit.map(|d| Arc::new(KeyedRateLimiter::new(d)));

        for w in 0..workers {
            shared_handles.push(tokio::spawn(Self::worker_thread(
                reqwest::Client::new(),
                rate_limit.clone(),
                request_receiver.clone(),
                response_sender.clone(),
                w,
            )));
        }

        (
            response_receiver,
            Self {
                workers,
                rate_limit,
                _use_dampener: use_dampener,
                shared_handles,
                _phantom: std::marker::PhantomData,
            },
        )
    }

    pub async fn worker_thread(
        client: reqwest::Client,
        rate_limit: Option<Arc<KeyedRateLimiter<usize>>>,
        request_receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<EventualRequest<T>>>>,
        response_sender: mpsc::Sender<EventualRequest<T>>,
        w: usize,
    ) {
        let mut retry: Option<EventualRequest<T>> = None;

        loop {
            let mut request = match retry {
                Some(r) => r,
                None => match request_receiver.lock().await.recv().await {
                    Some(req) => req,

                    // Channel closed, exit the worker thread
                    None => break,
                },
            };
            retry = None;

            if let Some(ref rl) = rate_limit {
                let sleep_for = rl.get_sleep_and_update(w);
                if !sleep_for.is_zero() {
                    tokio::time::sleep(sleep_for).await;
                }
            }

            request.time_request_start();
            let result = request.execute(&client).await;
            request.time_request_stop();
            match result {
                EventualRequestResult::Success => {
                    request.time_queue_start();
                    match response_sender.send(request).await {
                        Err(e) => {
                            eprintln!("Error sending response back from worker: {}", e.to_string())
                        }
                        _ => {}
                    }
                }
                EventualRequestResult::Retry => {
                    retry = Some(request);
                    continue;
                }
                EventualRequestResult::Error(e) => {
                    eprintln!("Error executing http request in worker {}: {}", w, e)
                }
            };
        }
    }

    /// Cap at rate_limit requests per second
    pub fn with_rate_limit(mut self, rate_limit: Decimal) -> Self {
        let desired_rate = rate_limit / Decimal::from_usize(self.workers).unwrap();
        if self.rate_limit.is_some() {
            self.rate_limit
                .as_mut()
                .unwrap()
                .set_desired_rate(desired_rate);
        }
        self
    }

    /// Shutdown the pool by waiting for all worker threads to finish
    /// The request_sender should be closed before calling this
    pub async fn shutdown(self) -> Result<(), tokio::task::JoinError> {
        // Wait for all worker threads to finish
        for handle in self.shared_handles {
            handle.await?;
        }
        // response_sender is automatically dropped here when self is consumed
        Ok(())
    }
}
