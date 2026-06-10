use std::fmt::Debug;

use async_trait::async_trait;
use serde_json::Value;

pub trait IntoValue: Send + Sync + Debug {
    fn into_value(self: Box<Self>) -> Value;
}

#[async_trait]
pub trait IntoValueAsync: Send + Sync + Debug {
    async fn to_value_async(&self) -> Value;
}

macro_rules! impl_into_value {
    ($($t:ty => $variant:expr),*) => {
        $(
            impl IntoValue for $t {
                fn into_value(self: Box<Self>) -> Value {
                    $variant(*self)
                }
            }
        )*
    };
}

impl_into_value! {
    i32 => |v| Value::Number(serde_json::Number::from(v)),
    i64 => |v| Value::Number(serde_json::Number::from(v)),
    u64 => |v| Value::Number(serde_json::Number::from(v)),
    f64 => |v| serde_json::Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null),
    bool => Value::Bool,
    String => Value::String,
    &str => |v:&str| Value::String(v.to_string()),
    () => |_| Value::Null
}

impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self: Box<Self>) -> Value {
        let array: Vec<Value> = self
            .into_iter()
            .map(|item| IntoValue::into_value(Box::new(item)))
            .collect();
        Value::Array(array)
    }
}

impl<T: IntoValue, const N: usize> IntoValue for [T; N] {
    fn into_value(self: Box<Self>) -> Value {
        let array: Vec<Value> = self
            .into_iter()
            .map(|item| IntoValue::into_value(Box::new(item)))
            .collect();
        Value::Array(array)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(_c: impl IntoValue) {}

    #[test]
    fn test_basic() {
        fx(123i32);
        fx(123i64);
        fx(123u64);
        fx(std::f64::consts::PI);
        fx(true);
        fx(false);
        fx("hello".to_string());
        fx("hello");
        fx(());
        fx(["some", "slice"]);
        fx(vec!["some", "slice"]);
    }

    #[test]
    fn test_finite_f64_into_value() {
        let v = IntoValue::into_value(Box::new(1.5f64));
        assert_eq!(v, Value::Number(serde_json::Number::from_f64(1.5).unwrap()));
    }

    #[test]
    fn test_non_finite_f64_into_value_is_null() {
        // `into_value` is infallible, and JSON has no NaN/Infinity, so
        // non-finite floats degrade to Null rather than panicking.
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(IntoValue::into_value(Box::new(v)), Value::Null);
        }
    }
}
