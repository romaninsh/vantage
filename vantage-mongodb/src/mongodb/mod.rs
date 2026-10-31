//! MongoDB data source for Vantage.
//!
//! Wraps the official `mongodb` crate. Uses `bson::Document` as the native
//! condition type — no SQL expressions involved.

pub mod impls;
mod streams;
pub mod topology;
pub(crate) mod tx;
pub mod view;

use std::sync::Arc;

use bson::doc;
use mongodb::Client;
use mongodb::options::ClientOptions;
use vantage_vista::{Transaction, TxKey};

pub use streams::{DEFAULT_STREAM_POOL_SIZE, STREAM_APP_NAME};
pub use topology::Topology;
pub use view::MongoView;

use streams::StreamClient;
use tx::{MongoBegin, MongoTx};

/// MongoDB data source. Cloneable (Arc-wrapped client). Wraps the client, or
/// one transaction in a session on it.
#[derive(Clone, Debug)]
pub struct MongoDB {
    client: Client,
    database: String,
    /// Identity shared by every clone: one transaction per datasource.
    key: Arc<()>,
    tx: Option<Arc<MongoTx>>,
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
    /// With the topology unknown, vistas over it join transactions, so on a
    /// standalone server a block's first operation fails instead of writing
    /// directly.
    pub fn new(client: Client, database: impl Into<String>) -> Self {
        Self::from_parts(client, database.into(), None)
    }

    fn from_parts(client: Client, database: String, options: Option<ClientOptions>) -> Self {
        Self {
            client,
            database,
            key: Arc::new(()),
            tx: None,
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

    /// Whether the deployment can run multi-document transactions. A
    /// standalone server can't; an `Unknown` topology is given the benefit
    /// of the doubt, so a server that turns out standalone fails the
    /// transaction's first operation instead.
    pub fn supports_transactions(&self) -> bool {
        self.topology.supports_transactions()
    }

    /// This datasource running its operations in a session's transaction
    /// on `tx`.
    pub fn in_transaction(&self, tx: &Transaction) -> vantage_core::Result<Self> {
        tx.check_open()?;
        let joined = tx.participant(self.tx_key(), || {
            MongoTx::new(MongoBegin::new(self.client.clone()), tx)
        })?;
        Ok(Self {
            tx: Some(joined),
            ..self.clone()
        })
    }

    pub fn tx_key(&self) -> TxKey {
        TxKey::of(&self.key)
    }

    pub(crate) fn tx(&self) -> Option<&MongoTx> {
        self.tx.as_deref()
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

#[cfg(all(test, feature = "vista"))]
mod tests {
    use std::time::Duration;

    use bson::oid::ObjectId;
    use ciborium::Value as CborValue;
    use tokio::time::timeout;
    use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
    use vantage_table::table::Table;
    use vantage_types::{EmptyEntity, Record};

    use super::*;

    /// The local replica set, taken for a standalone server by the `hello`
    /// reply one gives (no `setName`): a wrapped Vista over it writes
    /// directly, and the commit names its table unprotected.
    #[tokio::test]
    async fn standalone_is_direct() {
        let url = std::env::var("MONGODB_URL")
            .unwrap_or_else(|_| "mongodb://localhost:27017/?directConnection=true".into());
        let name = format!("vantage_standalone_{}", ObjectId::new().to_hex());
        let mut db = MongoDB::connect(&url, &name).await.unwrap();
        db.topology = Topology::from_hello(&doc! { "isWritablePrimary": true, "ok": 1 });
        assert!(!db.supports_transactions());

        // Nothing panics before the drop, so the database goes either way.
        let ran = timeout(Duration::from_secs(4), async {
            let table = Table::<MongoDB, EmptyEntity>::new("a", db.clone()).with_id_column("id");
            let a = db.vista_factory().from_table(table)?;
            let tx = Transaction::new();
            let row: Record<CborValue> = [("n".to_string(), CborValue::Integer(1.into()))]
                .into_iter()
                .collect();
            tx.wrap(&a).insert_value("x", &row).await?;
            let landed = a.get_value("x").await?.is_some();
            Ok::<_, vantage_core::VantageError>((landed, tx.commit().await?))
        })
        .await;
        db.database().drop().await.unwrap();
        let (landed, committed) = ran.expect("the standalone test ran out of time").unwrap();
        assert!(landed, "the direct write isn't visible before commit");
        assert_eq!(committed.unprotected, ["a"]);
        assert!(committed.written.is_empty());
    }
}
