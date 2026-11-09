pub mod impls;
pub(crate) mod references;
pub(crate) mod rollup;
pub(crate) mod tx;

use std::sync::{Arc, Mutex};

use surreal_client::SurrealClient;

use references::ReferenceLinks;
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
    /// The `REFERENCE` links [`SurrealDB::check_references`] found, shared by
    /// every clone.
    references: Arc<Mutex<ReferenceLinks>>,
}
