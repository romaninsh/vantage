use reqwest::{Client, Request, Response};
use rust_decimal::Decimal;
use std::future::Future;
use std::sync::Mutex;
use tokio::{sync::mpsc, task::JoinError};

use crate::{EventualRequestMatcher, HttpClientPool};

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + Sync + 'a>>;

struct Metadata {}

pub struct AwwPool {
    http_client_pool: HttpClientPool<Metadata>,
    eventual_request_matcher: EventualRequestMatcher<Metadata>,
    base_url: String,

    // Auth callbacks
    auth_acquire_fn: Option<
        std::sync::Arc<dyn Fn() -> BoxFuture<'static, Result<String, anyhow::Error>> + Send + Sync>,
    >,
    auth_apply_fn: Option<std::sync::Arc<dyn Fn(Request, &str) -> Request + Send + Sync>>,

    // Token pool
    auth_tokens: Mutex<Vec<String>>,
}

impl AwwPool {
    pub fn new(
        workers: usize,
        rate_limit: Option<Decimal>,
        use_dampener: bool,
        base_url: String,
    ) -> AwwPool {
        let (request_sender, request_receiver) = mpsc::channel(100);

        let (response_receiver, http_client_pool) =
            HttpClientPool::new(workers, rate_limit, use_dampener, request_receiver);

        Self {
            http_client_pool,
            eventual_request_matcher: EventualRequestMatcher::new(
                request_sender,
                response_receiver,
            ),
            base_url,
            auth_acquire_fn: None,
            auth_apply_fn: None,
            auth_tokens: Mutex::new(Vec::new()),
        }
    }

    /// pool.request(client.post(url).build());
    pub async fn request(&self, mut request: Request) -> anyhow::Result<Response> {
        // Apply auth if configured
        if let (Some(acquire_fn), Some(apply_fn)) = (&self.auth_acquire_fn, &self.auth_apply_fn) {
            let token = self.get_auth_token(acquire_fn).await?;
            request = apply_fn(request, &token);
        }

        self.eventual_request_matcher
            .send(request, None)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    pub async fn get(&self, path: &str) -> anyhow::Result<Response> {
        let full_url = if path.starts_with('/') {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/{}", self.base_url, path)
        };

        let mut request = Client::builder().build()?.get(&full_url).build()?;

        // Apply auth if configured
        if let (Some(acquire_fn), Some(apply_fn)) = (&self.auth_acquire_fn, &self.auth_apply_fn) {
            let token = self.get_auth_token(acquire_fn).await?;
            request = apply_fn(request, &token);
        }

        self.eventual_request_matcher
            .send(request, None)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    pub fn with_auth_callback<F, Fut, G>(
        mut self,
        n: usize,
        token_acquirer: F,
        request_modifier: G,
    ) -> AwwPool
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, anyhow::Error>> + Send + Sync + 'static,
        G: Fn(Request, &str) -> Request + Send + Sync + 'static,
    {
        // Pin the future inside the method
        let async_fn =
            move || Box::pin(token_acquirer()) as BoxFuture<'static, Result<String, anyhow::Error>>;

        self.auth_acquire_fn = Some(std::sync::Arc::new(async_fn));
        self.auth_apply_fn = Some(std::sync::Arc::new(request_modifier));
        self.auth_tokens = Mutex::new(Vec::with_capacity(n));
        self
    }

    async fn get_auth_token(
        &self,
        acquire_fn: &std::sync::Arc<
            dyn Fn() -> BoxFuture<'static, Result<String, anyhow::Error>> + Send + Sync,
        >,
    ) -> anyhow::Result<String> {
        // Check if we already have a cached token
        {
            let tokens = self.auth_tokens.lock().unwrap();
            if !tokens.is_empty() {
                return Ok(tokens[0].clone());
            }
        }

        // No cached token, acquire a new one
        let token = acquire_fn().await?;

        // Cache the token
        {
            let mut tokens = self.auth_tokens.lock().unwrap();
            if tokens.is_empty() {
                tokens.push(token.clone());
            }
        }

        Ok(token)
    }

    // Gracefully shuts down the AwwPool, ensuring all resources are cleaned up.
    pub async fn shutdown(self) -> Result<(), JoinError> {
        self.eventual_request_matcher.shutdown().await?;

        self.http_client_pool.shutdown().await?;

        Ok(())
    }
}
