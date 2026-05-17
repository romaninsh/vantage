use std::future::Future;
use std::pin::Pin;

use vantage_core::Result;

use crate::dio::Dio;
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

/// The five callback slots a Lens may hold. Each is independently
/// optional; the Lens treats absent slots as "use the default path"
/// (read from cache, write to master, etc.).
#[derive(Default)]
pub struct LensCallbacks {
    pub on_start: Option<DioCallback>,
    pub on_refresh: Option<DioCallback>,
    pub on_write: Option<DioWriteCallback>,
    pub on_event: Option<DioEventCallback>,
    pub on_query: Option<DioQueryCallback>,
}

/// Helper: wrap a user closure into a `DioCallback`. Accepts any closure
/// returning a future borrowing from `&Dio`.
///
/// Stage 1 ships the helper; later stages may add `on_start_fn`-style
/// builder shortcuts that call this internally.
pub fn boxed_dio_callback<F, Fut>(f: F) -> DioCallback
where
    F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    Box::new(move |dio| Box::pin(f(dio)))
}
