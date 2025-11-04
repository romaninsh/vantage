use serde_json::Value;
use std::fmt::{Debug, Formatter, Result};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

type DeferredFn<T> =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = IntoExpressive<T>> + Send>> + Send + Sync>;

pub enum IntoExpressive<T> {
    Scalar(Value),
    Nested(T),
    Deferred(DeferredFn<T>),
}

impl<T: Debug> Debug for IntoExpressive<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            IntoExpressive::Scalar(val) => f.debug_tuple("Scalar").field(val).finish(),
            IntoExpressive::Nested(val) => f.debug_tuple("Nested").field(val).finish(),
            IntoExpressive::Deferred(_) => f.debug_tuple("Deferred").field(&"<closure>").finish(),
        }
    }
}

pub trait Expressive<T>: Debug {
    fn expr(&self, template: &str, args: Vec<IntoExpressive<T>>) -> T;
}

impl<T: Clone> Clone for IntoExpressive<T> {
    fn clone(&self) -> Self {
        match self {
            IntoExpressive::Scalar(val) => IntoExpressive::Scalar(val.clone()),
            IntoExpressive::Nested(expr) => IntoExpressive::Nested(expr.clone()),
            IntoExpressive::Deferred(f) => IntoExpressive::Deferred(f.clone()),
        }
    }
}

// Macro for types that can be used directly with Value constructors
macro_rules! impl_scalar {
    ($($t:ty => $variant:path),* $(,)?) => {
        $(
            impl<T> From<$t> for IntoExpressive<T> {
                fn from(value: $t) -> Self {
                    IntoExpressive::Scalar($variant(value))
                }
            }
        )*
    };
}

// Macro for types that need .into() conversion
macro_rules! impl_scalar_into {
    ($($t:ty => $variant:path),* $(,)?) => {
        $(
            impl<T> From<$t> for IntoExpressive<T> {
                fn from(value: $t) -> Self {
                    IntoExpressive::Scalar($variant(value.into()))
                }
            }
        )*
    };
}

impl_scalar! {
    bool => Value::Bool,
    String => Value::String,
}

impl_scalar_into! {
    &str => Value::String,
    i8 => Value::Number,
    i16 => Value::Number,
    i32 => Value::Number,
    i64 => Value::Number,
    u8 => Value::Number,
    u16 => Value::Number,
    u32 => Value::Number,
}

impl<T> From<f64> for IntoExpressive<T> {
    fn from(value: f64) -> Self {
        IntoExpressive::Scalar(Value::Number(
            serde_json::Number::from_f64(value).unwrap_or_else(|| 0.into()),
        ))
    }
}

impl<T> From<Value> for IntoExpressive<T> {
    fn from(value: Value) -> Self {
        IntoExpressive::Scalar(value)
    }
}

impl<T, E> From<Arc<T>> for IntoExpressive<E>
where
    T: Into<IntoExpressive<E>> + Clone,
{
    fn from(arc: Arc<T>) -> Self {
        let value = arc.as_ref().clone();
        value.into()
    }
}

impl<T, E> From<&Arc<T>> for IntoExpressive<E>
where
    T: Into<IntoExpressive<E>> + Clone,
{
    fn from(arc: &Arc<T>) -> Self {
        let value = arc.as_ref().clone();
        value.into()
    }
}

impl<T> IntoExpressive<T> {
    pub fn nested(value: T) -> Self {
        IntoExpressive::Nested(value)
    }

    pub fn deferred<F>(f: F) -> Self
    where
        F: Fn() -> Pin<Box<dyn Future<Output = IntoExpressive<T>> + Send>> + Send + Sync + 'static,
    {
        IntoExpressive::Deferred(Arc::new(f))
    }
}

impl<T: Debug> IntoExpressive<T> {
    pub fn preview(&self) -> String {
        match self {
            IntoExpressive::Scalar(Value::String(s)) => format!("{:?}", s),
            IntoExpressive::Scalar(other) => format!("{}", other),
            IntoExpressive::Nested(expr) => format!("{:?}", expr),
            IntoExpressive::Deferred(_) => "**deferred()".to_string(),
        }
    }

    pub fn as_scalar(&self) -> Option<&Value> {
        match self {
            IntoExpressive::Scalar(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_nested(&self) -> Option<&T> {
        match self {
            IntoExpressive::Nested(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_deferred(&self) -> Option<&DeferredFn<T>> {
        match self {
            IntoExpressive::Deferred(f) => Some(f),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {}
