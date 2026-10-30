use std::any::Any;
use std::sync::Arc;

use async_trait::async_trait;
use vantage_core::Result;

/// Identifies a participant within one transaction by the address of a token
/// the caller keeps alive (typically the connection it belongs to).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TxKey(usize);

impl TxKey {
    pub fn of<T: ?Sized>(token: &Arc<T>) -> TxKey {
        TxKey(Arc::as_ptr(token) as *const () as usize)
    }
}

/// One backend's share of a transaction, finished together with the others.
#[async_trait]
pub trait TxParticipant: Any + Send + Sync {
    async fn commit(&self) -> Result<()>;
    async fn rollback(&self) -> Result<()>;
}
