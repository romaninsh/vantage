//! A separate driver client for change streams.
//!
//! An open change stream keeps a `getMore` in flight on the server for up to
//! a second at a time, and holds a pooled connection for all of it. Sharing
//! the main client's pool (10 connections by default) with a handful of live
//! tables leaves ordinary reads queueing behind the streams. Streams opened
//! through [`MongoDB::stream_collection`] therefore run on a client of their
//! own, built on the first watch from the options the main client was
//! connected with.

use std::sync::Arc;

#[cfg(feature = "vista")]
use bson::Document;
use mongodb::Client;
use mongodb::options::ClientOptions;
use tokio::sync::OnceCell;
#[cfg(feature = "vista")]
use vantage_core::{Result, error};

use super::MongoDB;

/// Connection pool size of the change-stream client: roughly how many
/// streams can wait on the server at once before they queue behind each
/// other.
pub const DEFAULT_STREAM_POOL_SIZE: u32 = 100;

/// Application name the change-stream client reports to the server, so its
/// connections and operations can be told apart in `$currentOp` and logs.
pub const STREAM_APP_NAME: &str = "vantage-mongodb-streams";

/// The change-stream side of a [`MongoDB`]. Clones share one client. Only
/// the `vista` feature opens streams.
#[derive(Clone, Debug)]
#[cfg_attr(not(feature = "vista"), allow(dead_code))]
pub(super) struct StreamClient {
    /// Options the main client was built from; `None` when the main client
    /// was handed in ready-made, in which case streams share it.
    options: Option<Arc<ClientOptions>>,
    pool_size: u32,
    client: Arc<OnceCell<Client>>,
}

impl StreamClient {
    pub(super) fn new(options: Option<ClientOptions>) -> Self {
        Self {
            options: options.map(Arc::new),
            pool_size: DEFAULT_STREAM_POOL_SIZE,
            client: Arc::new(OnceCell::new()),
        }
    }
}

impl MongoDB {
    /// Set the connection pool size of the client change streams run on
    /// (default [`DEFAULT_STREAM_POOL_SIZE`]). That client is built on the
    /// first watch and shared by every clone, so this must be called before
    /// any vista over this handle (or a clone of it) is watched; later calls
    /// have no effect. Handles built with [`new`](Self::new) open their
    /// streams on the client they were given, and ignore this setting.
    ///
    /// A size of 0 is raised to 1, the smallest pool the driver accepts.
    /// The stream client otherwise takes its options from the connection
    /// string, `minPoolSize` included (capped at this size).
    pub fn with_stream_pool_size(mut self, size: u32) -> Self {
        self.streams.pool_size = size.max(1);
        self
    }

    /// The pool size set for the change-stream client. It only takes effect
    /// on a handle built by [`connect`](Self::connect), and only until the
    /// first watch builds that client: a clone configured after that reports
    /// a size the shared client was not built with.
    pub fn stream_pool_size(&self) -> u32 {
        self.streams.pool_size
    }

    /// A collection handle for opening change streams on: on the dedicated
    /// stream client when this handle was built by
    /// [`connect`](Self::connect), on the main client otherwise.
    #[cfg(feature = "vista")]
    pub(crate) async fn stream_collection(
        &self,
        name: &str,
    ) -> Result<mongodb::Collection<Document>> {
        let Some(options) = &self.streams.options else {
            return Ok(self.doc_collection(name));
        };
        let client = self
            .streams
            .client
            .get_or_try_init(|| async {
                let mut options = ClientOptions::clone(options);
                let size = self.streams.pool_size;
                options.max_pool_size = Some(size);
                options.min_pool_size = options.min_pool_size.map(|min| min.min(size));
                options.app_name = Some(STREAM_APP_NAME.to_string());
                Client::with_options(options).map_err(|e| {
                    error!(
                        "Failed to build MongoDB change-stream client",
                        details = e.to_string()
                    )
                })
            })
            .await?;
        Ok(client.database(&self.database).collection(name))
    }
}
