//! [`Loader`]: how a Lens fills a Dio's cache from its master.

use std::future::Future;
use std::ops::Range;

use vantage_core::{Result, error};
use vantage_vista::{Capability, VistaCapabilities, VistaQuery};

use super::callbacks::{ChunkFn, CxFn, boxed_chunk, boxed_cx};
use super::chunk_sink::ChunkSink;
use super::loader_cx::LoaderCx;

/// How a Lens fills a Dio's cache.
///
/// - [`Loader::eager`] copies the whole set into the cache once, when the Dio
///   is built. Sceneries then read the cache.
/// - [`Loader::paged`] fetches the windows a table scenery asks for, as it
///   asks for them.
/// - [`Loader::paged(..).or_eager(..)`](Self::or_eager) pages a master that
///   advertises `Window` and copies a master that doesn't, so one Lens serves
///   both kinds of table.
///
/// A Lens with no loader serves whatever its cache already holds.
pub struct Loader {
    pub(crate) eager: Option<CxFn>,
    pub(crate) paged: Option<ChunkFn>,
    pub(crate) blocking: Option<bool>,
    /// `or_eager` on a loader with no paged half: reported by
    /// [`validate`](Self::validate) rather than silently replacing the start.
    or_eager_without_paged: bool,
}

impl Loader {
    /// Copy the whole set into the cache once per Dio. `start` hands the set
    /// to [`LoaderCx::reconcile`], or writes [`LoaderCx::cache`] directly. By
    /// default the Dio's build waits for it; see [`blocking`](Self::blocking).
    pub fn eager<F, Fut>(start: F) -> Self
    where
        F: Fn(LoaderCx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            eager: Some(boxed_cx(start)),
            ..Self::none()
        }
    }

    /// Fetch windows on demand. `chunk` receives the window, the requesting
    /// view's query (by value: the future outlives the call) and the sink it
    /// pushes rows into. It reports the grand total, when it learns one,
    /// through [`ChunkSink::set_total`].
    pub fn paged<F, Fut>(chunk: F) -> Self
    where
        F: Fn(LoaderCx, Range<usize>, VistaQuery, ChunkSink) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            paged: Some(boxed_chunk(chunk)),
            ..Self::none()
        }
    }

    /// Whether the Dio's build waits for the eager start (`true`, the default)
    /// or returns at once and lets the copy land in the background. Applies
    /// only to an eager start; on a paged loader without one it fails the
    /// Lens build.
    pub fn blocking(mut self, blocking: bool) -> Self {
        self.blocking = Some(blocking);
        self
    }

    /// Give a paged loader an eager start for masters that can't serve a
    /// window: a Dio whose master lacks `Window` is copied whole by `start`,
    /// and every other Dio pages. On a loader with no paged half it fails the
    /// Lens build.
    pub fn or_eager<F, Fut>(mut self, start: F) -> Self
    where
        F: Fn(LoaderCx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        if self.paged.is_none() {
            self.or_eager_without_paged = true;
        }
        self.eager = Some(boxed_cx(start));
        self
    }

    /// No loader: the cache is all there is.
    pub(crate) fn none() -> Self {
        Self {
            eager: None,
            paged: None,
            blocking: None,
            or_eager_without_paged: false,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.or_eager_without_paged {
            return Err(error!(
                "Loader::or_eager applies only to a paged loader",
                setting = "loader.or_eager"
            ));
        }
        if self.blocking.is_some() && self.eager.is_none() {
            return Err(error!(
                "Loader::blocking applies only to an eager start",
                setting = "loader.blocking"
            ));
        }
        Ok(())
    }

    /// Whether the eager start, when it runs, is awaited by the Dio's build.
    pub(crate) fn eager_blocking(&self) -> bool {
        self.blocking.unwrap_or(true)
    }

    /// Whether a Dio over a master with `caps` runs the eager start: always
    /// for an eager loader, and for a paged one only when the master can't
    /// serve a window.
    pub(crate) fn runs_eager_start(&self, caps: &VistaCapabilities) -> bool {
        self.eager.is_some() && (self.paged.is_none() || !caps.has(Capability::Window))
    }

    /// Whether table sceneries over a master with `caps` page lazily. The
    /// complement of [`runs_eager_start`](Self::runs_eager_start) for a
    /// loader that has both halves.
    pub(crate) fn pages(&self, caps: &VistaCapabilities) -> bool {
        self.paged.is_some() && (self.eager.is_none() || caps.has(Capability::Window))
    }
}
