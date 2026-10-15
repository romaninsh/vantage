use rust_decimal::Decimal;
use serde_json::Value;
use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: MockType,
    method_name: json,
    value_type: serde_json::Value,
    null_when: serde_json::Value::Null,
    type_variants: [String, Int, Float, Decimal, Bool, Null]
}

impl MockTypeVariants {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            Value::Number(n) if n.is_i64() => Some(MockTypeVariants::Int),
            Value::Number(n) if n.is_f64() => Some(MockTypeVariants::Float),
            Value::String(_) => Some(MockTypeVariants::String),
            Value::Bool(_) => Some(MockTypeVariants::Bool),
            Value::Object(obj) => {
                if obj.contains_key("decimal") {
                    Some(MockTypeVariants::Decimal)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl MockType for i64 {
    type Target = MockTypeIntMarker;

    fn to_json(&self) -> serde_json::Value {
        Value::Number(serde_json::Number::from(*self))
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64(),
            _ => None,
        }
    }
}

impl MockType for f64 {
    type Target = MockTypeFloatMarker;

    fn to_json(&self) -> serde_json::Value {
        serde_json::Number::from_f64(*self)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }
}

impl MockType for Decimal {
    type Target = MockTypeDecimalMarker;

    fn to_json(&self) -> serde_json::Value {
        // Store decimal as {"decimal": "decimal_string"} to avoid precision loss
        serde_json::json!({"decimal": self.to_string()})
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Object(obj) => {
                if let Some(Value::String(decimal_str)) = obj.get("decimal") {
                    decimal_str.parse().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl MockType for String {
    type Target = MockTypeStringMarker;

    fn to_json(&self) -> serde_json::Value {
        Value::String(self.clone())
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl MockType for bool {
    type Target = MockTypeBoolMarker;

    fn to_json(&self) -> serde_json::Value {
        Value::Bool(*self)
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}

impl MockType for &'static str {
    type Target = MockTypeStringMarker;

    fn to_json(&self) -> serde_json::Value {
        Value::String(self.to_string())
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(Box::leak(s.into_boxed_str())),
            _ => None,
        }
    }
}

impl<T> MockType for Option<T>
where
    T: MockType,
{
    type Target = T::Target;

    fn to_json(&self) -> serde_json::Value {
        match self {
            Some(value) => value.to_json(),
            None => Value::Null,
        }
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Null => Some(None),
            Value::String(_) | Value::Number(_) | Value::Object(_) => T::from_json(value).map(Some),
            _ => None,
        }
    }
}
