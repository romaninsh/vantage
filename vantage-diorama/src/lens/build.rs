use std::sync::Arc;

use tokio::runtime::Handle;

use crate::error::LensBuildError;

use super::{Lens, LensBuilder, LensCallbacks};

impl LensBuilder {
    /// Validate required state and produce the built [`Lens`].
    ///
    /// Stage 1 only asserts a cache backend is present. The actual
    /// runtime wiring (refresh task spawning, callback dispatch) lands
    /// in stages 3+.
    pub fn build(self) -> Result<Lens, LensBuildError> {
        if let Some(err) = self.deferred_cache_error {
            return Err(err);
        }
        let cache_source = self
            .cache_source
            .ok_or(LensBuildError::MissingCacheSource)?;
        let runtime = self
            .runtime
            .unwrap_or_else(|| Handle::try_current().expect("LensBuilder::build called outside a tokio runtime; supply one with .runtime(handle)"));

        let callbacks = LensCallbacks {
            on_start: self.on_start,
            on_refresh: self.on_refresh,
            on_write: self.on_write,
            on_event: self.on_event,
            on_query: self.on_query,
            total_provider: self.total_provider,
            on_load_chunk: self.on_load_chunk,
        };

        Ok(Lens {
            cache_source,
            callbacks: Arc::new(callbacks),
            defaults: self.defaults,
            runtime,
        })
    }
}
