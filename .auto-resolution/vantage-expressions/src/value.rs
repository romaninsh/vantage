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
    f64 => |v| Value::Number(serde_json::Number::from_f64(v).unwrap()),
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
}
