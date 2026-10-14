//! The boxed forms of the closures a [`LensBuilder`](super::LensBuilder)
//! takes. Each receives an owned [`LoaderCx`] and returns a `'static` future,
//! so one boxed closure serves every Dio of the Lens.

use std::future::Future;
use std::ops::Range;
use std::pin::Pin;

use vantage_core::Result;
use vantage_vista::VistaQuery;

use super::chunk_sink::ChunkSink;
use super::loader::Loader;
use super::loader_cx::LoaderCx;
use super::refresher::Refresher;
use crate::ops::{ChangeFlash, FlashOutcome};

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'static>>;

/// An eager start or a refresh: one call per Dio, with the Dio's context.
pub(crate) type CxFn = Box<dyn Fn(LoaderCx) -> BoxFuture<()> + Send + Sync>;

/// A paged loader: fetch `range` of the view's query and push it into the sink.
pub(crate) type ChunkFn =
    Box<dyn Fn(LoaderCx, Range<usize>, VistaQuery, ChunkSink) -> BoxFuture<()> + Send + Sync>;

/// A writer: carry one flash to wherever the Dio's writes go.
pub(crate) type WriteFn =
    Box<dyn Fn(LoaderCx, ChangeFlash) -> BoxFuture<FlashOutcome> + Send + Sync>;

pub(crate) fn boxed_cx<F, Fut>(f: F) -> CxFn
where
    F: Fn(LoaderCx) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |cx| Box::pin(f(cx)))
}

pub(crate) fn boxed_chunk<F, Fut>(f: F) -> ChunkFn
where
    F: Fn(LoaderCx, Range<usize>, VistaQuery, ChunkSink) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |cx, range, query, sink| Box::pin(f(cx, range, query, sink)))
}

pub(crate) fn boxed_writer<F, Fut>(f: F) -> WriteFn
where
    F: Fn(LoaderCx, ChangeFlash) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<FlashOutcome>> + Send + 'static,
{
    Box::new(move |cx, flash| Box::pin(f(cx, flash)))
}

/// Everything a built Lens calls back into. Absent parts take the default
/// path: no loader means the cache is all there is, no refresher means no
/// timer, and no writer means writes go to the master.
pub(crate) struct LensCallbacks {
    pub(crate) loader: Loader,
    pub(crate) refresher: Option<Refresher>,
    pub(crate) writer: Option<WriteFn>,
}
