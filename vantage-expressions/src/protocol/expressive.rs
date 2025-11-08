use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use vantage_core::Result;

use crate::expression::owned::Expression;

pub type DeferredFuture<T> = Pin<Box<dyn Future<Output = Result<ExpressiveEnum<T>>> + Send>>;
pub type DeferredCallback<T> = Arc<dyn Fn() -> DeferredFuture<T> + Send + Sync>;

#[derive(Clone)]
pub struct DeferredFn<T> {
    func: DeferredCallback<T>,
}

impl<T> DeferredFn<T> {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> DeferredFuture<T> + Send + Sync + 'static,
    {
        Self { func: Arc::new(f) }
    }

    pub async fn call(&self) -> Result<ExpressiveEnum<T>> {
        (self.func)().await
    }

    /// Create a DeferredFn that reads from an Arc<Mutex<T>> when executed
    pub fn from_mutex<U>(mutex: Arc<Mutex<U>>) -> Self
    where
        U: Clone + Into<T> + Send + 'static,
        T: Send + 'static,
    {
        Self::new(move || {
            let mutex = mutex.clone();
            Box::pin(async move {
                let value = mutex.lock().unwrap().clone();
                Ok(ExpressiveEnum::Scalar(value.into()))
            })
        })
    }

    /// Create a DeferredFn from an async function, hiding the Pin logic
    pub fn from_fn<F, Fut, U>(f: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<U>> + Send + 'static,
        U: Into<T> + Send + 'static,
        T: Send + 'static,
    {
        let f = Arc::new(f);
        Self::new(move || {
            let f = f.clone();
            Box::pin(async move {
                match f().await {
                    Ok(result) => Ok(ExpressiveEnum::Scalar(result.into())),
                    Err(e) => Err(e),
                }
            })
        })
    }
}

impl<T: Debug + std::fmt::Display> Debug for DeferredFn<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_tuple("DeferredFn").field(&"<closure>").finish()
    }
}

pub enum ExpressiveEnum<T> {
    Scalar(T),
    Nested(Expression<T>),
    Deferred(DeferredFn<T>),
}

impl<T: Debug + std::fmt::Display> Debug for ExpressiveEnum<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            ExpressiveEnum::Scalar(val) => f.debug_tuple("Scalar").field(val).finish(),
            ExpressiveEnum::Nested(val) => f.debug_tuple("Nested").field(val).finish(),
            ExpressiveEnum::Deferred(deferred) => {
                f.debug_tuple("Deferred").field(deferred).finish()
            }
        }
    }
}

pub trait Expressive<T> {
    fn expr(&self) -> Expression<T>;
}

impl<T: Clone> Clone for ExpressiveEnum<T> {
    fn clone(&self) -> Self {
        match self {
            ExpressiveEnum::Scalar(val) => ExpressiveEnum::Scalar(val.clone()),
            ExpressiveEnum::Nested(expr) => ExpressiveEnum::Nested(expr.clone()),
            ExpressiveEnum::Deferred(f) => ExpressiveEnum::Deferred(f.clone()),
        }
    }
}

impl<T> ExpressiveEnum<T> {
    pub fn nested(value: Expression<T>) -> Self {
        ExpressiveEnum::Nested(value)
    }

    pub fn deferred<F>(f: F) -> Self
    where
        F: Fn() -> DeferredFuture<T> + Send + Sync + 'static,
    {
        ExpressiveEnum::Deferred(DeferredFn::new(f))
    }
}

impl<T: std::fmt::Debug + std::fmt::Display> ExpressiveEnum<T> {
    pub fn preview(&self) -> String {
        match self {
            ExpressiveEnum::Scalar(val) => format!("{}", val),
            ExpressiveEnum::Nested(expr) => format!("{:?}", expr),
            ExpressiveEnum::Deferred(_) => "**deferred()".to_string(),
        }
    }
}

// Enable conversion from DeferredFn to ExpressiveEnum
impl<T> From<DeferredFn<T>> for ExpressiveEnum<T> {
    fn from(deferred: DeferredFn<T>) -> Self {
        ExpressiveEnum::Deferred(deferred)
    }
}

// Enable conversion from closures to ExpressiveEnum::Deferred
impl<T, F> From<F> for ExpressiveEnum<T>
where
    F: Fn() -> DeferredFuture<T> + Send + Sync + 'static,
{
    fn from(closure: F) -> Self {
        ExpressiveEnum::Deferred(DeferredFn::new(closure))
    }
}

#[cfg(test)]
mod tests {}
