//! MongoDB data source for Vantage.
//!
//! Wraps the official `mongodb` crate. Uses `bson::Document` as the native
//! condition type — no SQL expressions involved.

pub mod impls;
pub mod topology;
pub mod view;

use std::sync::Arc;

use bson::doc;
use mongodb::Client;

pub use topology::Topology;
pub use view::MongoView;

/// MongoDB data source. Cloneable (Arc-wrapped client).
#[derive(Clone, Debug)]
pub struct MongoDB {
    client: Client,
    database: String,
    topology: Topology,
    /// Pipeline the tables over this source read instead of their collection.
    view: Option<Arc<MongoView>>,
}

impl MongoDB {
    /// Create from an existing `mongodb::Client` and database name. The
    /// topology is not probed, so it stays `Unknown`.
    pub fn new(client: Client, database: impl Into<String>) -> Self {
        Self {
            client,
            database: database.into(),
            topology: Topology::Unknown,
            view: None,
        }
    }

    /// Connect using a MongoDB connection string, probing the deployment
    /// topology once with `hello`. The probe waits for server selection, so
    /// against an unreachable server this blocks for the client's
    /// server-selection timeout (30 s by default, `serverSelectionTimeoutMS`
    /// in the URI) and then leaves the topology `Unknown`. Use
    /// [`new`](Self::new) to build a handle without touching the network.
    pub async fn connect(uri: &str, database: impl Into<String>) -> vantage_core::Result<Self> {
        let client = Client::with_uri_str(uri).await.map_err(|e| {
            vantage_core::error!("Failed to connect to MongoDB", details = e.to_string())
        })?;
        let mut mongo = Self::new(client, database);
        mongo.topology = match mongo.database().run_command(doc! { "hello": 1 }).await {
            Ok(reply) => Topology::from_hello(&reply),
            Err(_) => Topology::Unknown,
        };
        Ok(mongo)
    }

    /// The deployment topology detected at connect time.
    pub fn topology(&self) -> Topology {
        self.topology
    }

    /// Whether the deployment can serve change streams.
    pub fn supports_change_streams(&self) -> bool {
        self.topology.supports_change_streams()
    }

    /// The underlying driver client, for session-bound operations.
    #[cfg(feature = "vista")]
    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    /// Get the underlying `mongodb::Database` handle.
    pub fn database(&self) -> mongodb::Database {
        self.client.database(&self.database)
    }

    /// Get a collection handle.
    pub fn collection<T: Send + Sync>(&self, name: &str) -> mongodb::Collection<T> {
        self.database().collection(name)
    }

    /// Get a collection handle for raw BSON documents.
    pub fn doc_collection(&self, name: &str) -> mongodb::Collection<bson::Document> {
        self.database().collection(name)
    }
}
