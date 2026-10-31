pub mod impls;
pub(crate) mod tx;

use std::sync::Arc;

use surreal_client::SurrealClient;

use tx::SurrealTx;

/// A SurrealDB datasource. Clones share one connection, or one transaction
/// on a connection of its own.
#[derive(Clone)]
pub struct SurrealDB {
    inner: Arc<tokio::sync::Mutex<SurrealClient>>,
    /// Identity shared by every clone: one transaction per datasource.
    key: Arc<()>,
    tx: Option<Arc<SurrealTx>>,
    transactions: bool,
}
