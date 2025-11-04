use std::fmt::{Debug, Formatter, Result};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::expression::owned::Expression;

type DeferredFn<T> =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ExpressiveEnum<T>> + Send>> + Send + Sync>;

pub enum ExpressiveEnum<T> {
    Scalar(T),
    Nested(Expression<T>),
    Deferred(DeferredFn<T>),
}

impl<T: Debug + std::fmt::Display> Debug for ExpressiveEnum<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            ExpressiveEnum::Scalar(val) => f.debug_tuple("Scalar").field(val).finish(),
            ExpressiveEnum::Nested(val) => f.debug_tuple("Nested").field(val).finish(),
            ExpressiveEnum::Deferred(_) => f.debug_tuple("Deferred").field(&"<closure>").finish(),
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
        F: Fn() -> Pin<Box<dyn Future<Output = ExpressiveEnum<T>> + Send>> + Send + Sync + 'static,
    {
        ExpressiveEnum::Deferred(Arc::new(f))
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

#[cfg(test)]
mod tests {}
