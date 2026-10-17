pub mod impls;

use std::sync::Arc;

use surreal_client::SurrealClient;

/// A SurrealDB datasource. Clones share one connection.
#[derive(Clone)]
pub struct SurrealDB {
    inner: Arc<tokio::sync::Mutex<SurrealClient>>,
}
