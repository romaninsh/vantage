use std::sync::Arc;

use tokio::runtime::Handle;

use crate::error::LensBuildError;

use super::{Lens, LensBuilder, LensCallbacks};

impl LensBuilder {
    /// Validate required state and produce the built [`Lens`].
    ///
    /// The Lens carries caching strategy and any explicitly-registered
    /// callbacks; two-pass augmentation is configured on the Dio via
    /// [`Dio::augment`](crate::Dio::augment), not here.
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
            on_flash: self.on_flash,
            on_event: self.on_event,
            total_provider: self.total_provider,
            on_load_chunk: self.on_load_chunk,
            on_list_page: self.on_list_page,
            on_load_detail: self.on_load_detail,
        };

        Ok(Lens {
            cache_source,
            callbacks: Arc::new(callbacks),
            defaults: self.defaults,
            runtime,
            activity: self.activity,
        })
    }
}
