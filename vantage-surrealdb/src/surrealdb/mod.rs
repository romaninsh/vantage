pub mod impls;

use std::sync::Arc;

use surreal_client::SurrealClient;

// Create a wrapper for shared SurrealDB state
#[derive(Clone)]
pub struct SurrealDB {
    inner: Arc<tokio::sync::Mutex<SurrealClient>>,
}
