//! The client's auth token: acquired lazily on the first attempt that needs
//! it, cached for every later attempt, and re-acquired once per call on a
//! `401`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

/// Future returned by the auth token refresher.
pub type AuthFuture = Pin<Box<dyn Future<Output = Result<String>> + Send>>;
/// Acquire (or re-acquire) an auth token — called lazily and on `401`.
pub type AuthRefresher = Arc<dyn Fn() -> AuthFuture + Send + Sync>;

pub(super) struct AuthState {
    pub(super) token: RwLock<Option<String>>,
    pub(super) refresh: AuthRefresher,
    pub(super) header: String,
    pub(super) scheme: String,
}

impl AuthState {
    /// The cached token, acquiring one if this is the first request.
    pub(super) async fn current(&self) -> Result<String> {
        if let Some(t) = self.token.read().await.clone() {
            return Ok(t);
        }
        self.reacquire().await
    }

    /// Discard the cached token and fetch a fresh one.
    pub(super) async fn reacquire(&self) -> Result<String> {
        let t = (self.refresh)().await?;
        *self.token.write().await = Some(t.clone());
        Ok(t)
    }
}
