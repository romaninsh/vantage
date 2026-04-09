//! MongoDB data source for Vantage.
//!
//! Wraps the official `mongodb` crate. Uses `bson::Document` as the native
//! condition type — no SQL expressions involved.

pub mod impls;

use mongodb::Client;

/// MongoDB data source. Cloneable (Arc-wrapped client).
#[derive(Clone, Debug)]
pub struct MongoDB {
    client: Client,
    database: String,
}

impl MongoDB {
    /// Create from an existing `mongodb::Client` and database name.
    pub fn new(client: Client, database: impl Into<String>) -> Self {
        Self {
            client,
            database: database.into(),
        }
    }

    /// Connect using a MongoDB connection string.
    pub async fn connect(uri: &str, database: impl Into<String>) -> vantage_core::Result<Self> {
        let client = Client::with_uri_str(uri).await.map_err(|e| {
            vantage_core::error!("Failed to connect to MongoDB", details = e.to_string())
        })?;
        Ok(Self::new(client, database))
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
