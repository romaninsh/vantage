use std::future::Future;
use std::ops::Range;
use std::pin::Pin;

use vantage_core::Result;

use crate::dio::Dio;
use crate::lens::chunk_sink::ChunkSink;
use crate::ops::{ChangeEvent, QueryDescriptor, WriteOp};

/// Future returned by a Dio callback. Borrows from the supplied `&Dio`.
pub type DioCallbackFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// Callback shape: borrow `&Dio`, return a borrowed future. Box the
/// closure once at registration; the HRTB lets a single boxed closure
/// be invoked against any `&Dio` lifetime.
pub type DioCallback =
    Box<dyn for<'a> Fn(&'a Dio) -> DioCallbackFuture<'a> + Send + Sync + 'static>;

pub type DioWriteCallback =
    Box<dyn for<'a> Fn(&'a Dio, WriteOp) -> DioCallbackFuture<'a> + Send + Sync + 'static>;

pub type DioEventCallback =
    Box<dyn for<'a> Fn(&'a Dio, ChangeEvent) -> DioCallbackFuture<'a> + Send + Sync + 'static>;

pub type DioQueryCallback =
    Box<dyn for<'a> Fn(&'a Dio, QueryDescriptor) -> DioCallbackFuture<'a> + Send + Sync + 'static>;

/// Future returned by a [`DioTotalProviderCallback`]. Carries a row
/// count back to the Scenery; runs once per scenery open and is
/// cached for the scenery's lifetime.
pub type DioTotalProviderFuture<'a> = Pin<Box<dyn Future<Output = Result<usize>> + Send + 'a>>;

pub type DioTotalProviderCallback =
    Box<dyn for<'a> Fn(&'a Dio) -> DioTotalProviderFuture<'a> + Send + Sync + 'static>;

/// Callback that fetches a contiguous range of rows from the master
/// and pushes them into the Scenery via [`ChunkSink::push`]. Returns
/// `Ok(())` once it is done pushing for this invocation.
pub type DioLoadChunkCallback = Box<
    dyn for<'a> Fn(&'a Dio, Range<usize>, ChunkSink) -> DioCallbackFuture<'a>
        + Send
        + Sync
        + 'static,
>;

/// The callback slots a Lens may hold. Each is independently
/// optional; the Lens treats absent slots as "use the default path"
/// (read from cache, write to master, etc.).
#[derive(Default)]
pub struct LensCallbacks {
    pub on_start: Option<DioCallback>,
    pub on_refresh: Option<DioCallback>,
    pub on_write: Option<DioWriteCallback>,
    pub on_event: Option<DioEventCallback>,
    pub on_query: Option<DioQueryCallback>,
    pub total_provider: Option<DioTotalProviderCallback>,
    pub on_load_chunk: Option<DioLoadChunkCallback>,
}

/// Wrap a user closure into a [`DioCallback`].
///
/// The canonical user pattern is `move |dio| { let dio = dio.clone();
/// async move { ... dio.cache().insert_values(...).await } }` —
/// cloning the Dio inside the closure produces a `'static` future,
/// which avoids the lifetime gymnastics of borrowing `&Dio` across an
/// await.
pub fn boxed_dio_callback<F, Fut>(f: F) -> DioCallback
where
    F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |dio| Box::pin(f(dio)))
}

/// Wrap a user closure into a [`DioWriteCallback`].
pub fn boxed_dio_write_callback<F, Fut>(f: F) -> DioWriteCallback
where
    F: for<'a> Fn(&'a Dio, WriteOp) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |dio, op| Box::pin(f(dio, op)))
}

/// Wrap a user closure into a [`DioEventCallback`].
pub fn boxed_dio_event_callback<F, Fut>(f: F) -> DioEventCallback
where
    F: for<'a> Fn(&'a Dio, ChangeEvent) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |dio, ev| Box::pin(f(dio, ev)))
}

/// Wrap a user closure into a [`DioQueryCallback`].
pub fn boxed_dio_query_callback<F, Fut>(f: F) -> DioQueryCallback
where
    F: for<'a> Fn(&'a Dio, QueryDescriptor) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |dio, q| Box::pin(f(dio, q)))
}

/// Wrap a user closure into a [`DioTotalProviderCallback`].
pub fn boxed_total_provider_callback<F, Fut>(f: F) -> DioTotalProviderCallback
where
    F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<usize>> + Send + 'static,
{
    Box::new(move |dio| Box::pin(f(dio)))
}

/// Wrap a user closure into a [`DioLoadChunkCallback`].
pub fn boxed_load_chunk_callback<F, Fut>(f: F) -> DioLoadChunkCallback
where
    F: for<'a> Fn(&'a Dio, Range<usize>, ChunkSink) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |dio, range, sink| Box::pin(f(dio, range, sink)))
}
