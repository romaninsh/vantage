//! [`LensBuilder`]: the one way to assemble a [`Lens`].

use std::future::Future;
use std::sync::Arc;

use tokio::runtime::Handle;
use vantage_core::{Result, error};

use super::callbacks::{LensCallbacks, WriteFn, boxed_writer};
use super::registry::DioRegistry;
use super::{Cache, Lens, Loader, LoaderCx, Refresher, Tuning};
use crate::ops::{ChangeFlash, FlashOutcome};

/// Collects a Lens's settings; [`build`](Self::build) validates them.
/// Start one with [`Lens::builder`].
pub struct LensBuilder {
    cache: Option<Cache>,
    loader: Option<Loader>,
    refresher: Option<Refresher>,
    writer: Option<WriteFn>,
    tuning: Tuning,
    runtime: Option<Handle>,
    debug: crate::debug::DebugTap,
}

impl LensBuilder {
    pub(crate) fn new() -> Self {
        Self {
            cache: None,
            loader: None,
            refresher: None,
            writer: None,
            tuning: Tuning::default(),
            runtime: None,
            debug: crate::debug::DebugTap::off(),
        }
    }

    /// Where every Dio of the Lens keeps its rows. Required.
    pub fn cache(mut self, cache: Cache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// How each Dio's cache is filled from its master. Without one, the cache
    /// is all there is.
    pub fn loader(mut self, loader: Loader) -> Self {
        self.loader = Some(loader);
        self
    }

    /// How, and how often, each Dio re-reads its master.
    pub fn refresher(mut self, refresher: Refresher) -> Self {
        self.refresher = Some(refresher);
        self
    }

    /// Route every write through `f` instead of the master. `Ok` confirms the
    /// write and keeps the staged cache value, or moves it onto the id the
    /// outcome reports; `Err`, or a panic, rolls it back. A writer makes every
    /// Dio of the Lens writable whatever its master allows.
    ///
    /// A Dio runs its writes one at a time, so the writer must not write
    /// through the Dio it is serving: a `flash` (or facade write) on that Dio
    /// from inside `f` returns an error naming the Dio. Write elsewhere, or
    /// through `cx.cache()`. A task the writer spawns and awaits isn't
    /// recognised and would wait forever.
    pub fn writer<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(LoaderCx, ChangeFlash) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<FlashOutcome>> + Send + 'static,
    {
        self.writer = Some(boxed_writer(f));
        self
    }

    pub fn tuning(mut self, tuning: Tuning) -> Self {
        self.tuning = tuning;
        self
    }

    /// The runtime the Lens spawns its tasks on. Without one, `build` takes
    /// the current runtime, and fails outside one.
    pub fn runtime(mut self, handle: Handle) -> Self {
        self.runtime = Some(handle);
        self
    }

    /// Enable the debug stream for every Dio this Lens builds. `name` is the
    /// datasource name stamped as `ds=` on every line of the
    /// `vantage_diorama::debug` target. Off by default.
    #[cfg(feature = "diagnostics")]
    pub fn debug_datasource(mut self, name: impl Into<String>) -> Self {
        self.debug = crate::debug::DebugTap::for_datasource(name);
        self
    }

    /// Carry another Lens's debug stream, for a Lens that serves Dios on its
    /// behalf.
    pub(crate) fn tap(mut self, tap: crate::debug::DebugTap) -> Self {
        self.debug = tap;
        self
    }

    /// Validate the settings and build the Lens. Every problem is an error
    /// naming the setting; this never panics.
    pub fn build(self) -> Result<Arc<Lens>> {
        let Some(cache) = self.cache else {
            return Err(error!("a Lens needs a cache", setting = "cache"));
        };
        let runtime = match self.runtime {
            Some(handle) => handle,
            None => Handle::try_current().map_err(|e| {
                error!(
                    "a Lens needs a tokio runtime: none was given and none is current",
                    setting = "runtime",
                    detail = e.to_string()
                )
            })?,
        };
        let loader = self.loader.unwrap_or_else(Loader::none);
        loader.validate()?;
        if let Some(refresher) = &self.refresher {
            refresher.validate()?;
        }
        self.tuning.validate()?;
        let cache = cache.open()?;

        Ok(Arc::new(Lens {
            cache,
            callbacks: LensCallbacks {
                loader,
                refresher: self.refresher,
                writer: self.writer,
            },
            tuning: self.tuning,
            runtime,
            debug: self.debug,
            dios: DioRegistry::default(),
        }))
    }
}
