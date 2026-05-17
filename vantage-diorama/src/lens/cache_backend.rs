use async_trait::async_trait;
use vantage_core::Result;
use vantage_vista::Vista;

/// Multi-table storage backing a [`Lens`](super::Lens).
///
/// Each Dio under a given Lens claims one table (named after its master
/// vista by default) within this backend. The backend produces a Vista
/// onto that table on demand; reads, writes, and indexes go through the
/// returned Vista.
///
/// Stage 1 defines the trait surface only. Stage 2 ships the first impl
/// (redb), and later stages may add in-memory, sqlite, or remote variants.
#[async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    /// Open (or create) the named cache table and return a Vista pointing
    /// at it. Implementations may cache the returned Vista internally so
    /// repeat calls with the same name share storage.
    async fn open_table(&self, name: &str) -> Result<Vista>;

    /// Short human label for diagnostics (e.g. `"redb"`, `"memory"`).
    fn name(&self) -> &'static str {
        "unknown"
    }
}
