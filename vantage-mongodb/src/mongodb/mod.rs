//! MongoDB data source for Vantage.
//!
//! Wraps the official `mongodb` crate. Uses `bson::Document` as the native
//! condition type — no SQL expressions involved.

pub mod impls;
mod streams;
pub mod topology;
pub mod view;

use std::sync::Arc;

use bson::doc;
use mongodb::Client;
use mongodb::options::ClientOptions;

pub use streams::{DEFAULT_STREAM_POOL_SIZE, STREAM_APP_NAME};
pub use topology::Topology;
pub use view::MongoView;

use streams::StreamClient;

/// MongoDB data source. Cloneable (Arc-wrapped client).
#[derive(Clone, Debug)]
pub struct MongoDB {
    client: Client,
    database: String,
    topology: Topology,
    /// Pipeline the tables over this source read instead of their collection.
    view: Option<Arc<MongoView>>,
    /// Whether the tables over this source are server-side views
    /// (`db.createView`), which serve reads only.
    server_view: bool,
    /// Client change streams open on, kept apart from reads and writes.
    streams: StreamClient,
}

impl MongoDB {
    /// Create from an existing `mongodb::Client` and database name. The
    /// topology is not probed, so it stays `Unknown`. Change streams share
    /// `client` with reads and writes, so each open stream holds one of its
    /// pooled connections; size its `max_pool_size` for that, or use
    /// [`connect`](Self::connect), which gives streams a client of their own.
    pub fn new(client: Client, database: impl Into<String>) -> Self {
        Self::from_parts(client, database.into(), None)
    }

    fn from_parts(client: Client, database: String, options: Option<ClientOptions>) -> Self {
        Self {
            client,
            database,
            topology: Topology::Unknown,
            view: None,
            server_view: false,
            streams: StreamClient::new(options),
        }
    }

    /// Connect using a MongoDB connection string, probing the deployment
    /// topology once with `hello`. The probe waits for server selection, so
    /// against an unreachable server this blocks for the client's
    /// server-selection timeout (30 s by default, `serverSelectionTimeoutMS`
    /// in the URI) and then leaves the topology `Unknown`. Use
    /// [`new`](Self::new) to build a handle without touching the network.
    ///
    /// Change streams run on a second client built from the same URI on the
    /// first watch; see [`with_stream_pool_size`](Self::with_stream_pool_size).
    pub async fn connect(uri: &str, database: impl Into<String>) -> vantage_core::Result<Self> {
        let connect_error = |e: mongodb::error::Error| {
            vantage_core::error!("Failed to connect to MongoDB", details = e.to_string())
        };
        let options = ClientOptions::parse(uri).await.map_err(connect_error)?;
        let client = Client::with_options(options.clone()).map_err(connect_error)?;
        let mut mongo = Self::from_parts(client, database.into(), Some(options));
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
