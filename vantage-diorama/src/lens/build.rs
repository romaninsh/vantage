use std::sync::Arc;

use tokio::runtime::Handle;
use vantage_vista_factory::VistaCatalog;

use crate::augment::Augmentation;
use crate::dio::Dio;
use crate::dio::augment_passes;
use crate::error::LensBuildError;

use super::callbacks::{
    DioCallback, DioListPageCallback, DioLoadDetailCallback, boxed_dio_callback,
    boxed_list_page_callback, boxed_load_detail_callback,
};
use super::{Lens, LensBuilder, LensCallbacks};

impl LensBuilder {
    /// Validate required state and produce the built [`Lens`].
    ///
    /// When [`augment`](Self::augment) registered any augmentations and no
    /// explicit two-pass callbacks were supplied, this synthesizes them: a
    /// capability-aware list pass and an augmentation detail pass
    /// (`synth_list_page` / `synth_load_detail`). Registering the detail pass
    /// is what engages two-pass loading downstream.
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

        let mut on_list_page = self.on_list_page;
        let mut on_load_detail = self.on_load_detail;
        let mut on_refresh = self.on_refresh;

        if !self.augmentations.is_empty() && on_load_detail.is_none() {
            let catalog = self.catalog.ok_or(LensBuildError::MissingCatalog)?;
            let augmentations = Arc::new(self.augmentations);

            // Each pass is synthesized only if the caller didn't supply one, so an
            // app can still hand-roll any single pass while reusing the rest.
            if on_list_page.is_none() {
                on_list_page = Some(synth_list_page());
            }
            if on_refresh.is_none() {
                on_refresh = Some(synth_refresh());
            }
            on_load_detail = Some(synth_load_detail(catalog, augmentations));
        }

        let callbacks = LensCallbacks {
            on_start: self.on_start,
            on_refresh,
            on_write: self.on_write,
            on_event: self.on_event,
            on_query: self.on_query,
            total_provider: self.total_provider,
            on_load_chunk: self.on_load_chunk,
            on_list_page,
            on_load_detail,
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

/// Capability-aware list pass for an augmented Dio — delegates to the shared
/// [`augment_passes::list_page`].
fn synth_list_page() -> DioListPageCallback {
    boxed_list_page_callback(move |dio: &Dio, desc| {
        let dio = dio.clone();
        async move { augment_passes::list_page(&dio, desc).await }
    })
}

/// Augmentation detail pass — delegates to [`augment_passes::load_detail_with`],
/// capturing the Lens-level catalog + augmentations.
fn synth_load_detail(
    catalog: Arc<VistaCatalog>,
    augmentations: Arc<Vec<Augmentation>>,
) -> DioLoadDetailCallback {
    boxed_load_detail_callback(move |dio: &Dio, id| {
        let dio = dio.clone();
        let catalog = catalog.clone();
        let augmentations = augmentations.clone();
        async move { augment_passes::load_detail_with(&dio, id, &catalog, &augmentations).await }
    })
}

/// Augmentation refresh — delegates to [`augment_passes::refresh`].
fn synth_refresh() -> DioCallback {
    boxed_dio_callback(move |dio: &Dio| {
        let dio = dio.clone();
        async move { augment_passes::refresh(&dio).await }
    })
}
