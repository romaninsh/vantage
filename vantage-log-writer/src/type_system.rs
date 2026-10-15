use serde_json::Value;
use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: JsonType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [String, Int, Float, Bool, Object, Array, Null]
}

impl JsonTypeVariants {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            Value::Null => Some(JsonTypeVariants::Null),
            Value::Bool(_) => Some(JsonTypeVariants::Bool),
            Value::Number(n) if n.is_i64() || n.is_u64() => Some(JsonTypeVariants::Int),
            Value::Number(_) => Some(JsonTypeVariants::Float),
            Value::String(_) => Some(JsonTypeVariants::String),
            Value::Array(_) => Some(JsonTypeVariants::Array),
            Value::Object(_) => Some(JsonTypeVariants::Object),
        }
    }
}

impl JsonType for String {
    type Target = JsonTypeStringMarker;

    fn to_json(&self) -> Value {
        Value::String(self.clone())
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl JsonType for i64 {
    type Target = JsonTypeIntMarker;

    fn to_json(&self) -> Value {
        Value::Number(serde_json::Number::from(*self))
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64(),
            _ => None,
        }
    }
}

impl JsonType for f64 {
    type Target = JsonTypeFloatMarker;

    fn to_json(&self) -> Value {
        serde_json::Number::from_f64(*self)
            .map(Value::Number)
            .unwrap_or(Value::Null)
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_f64(),
            _ => None,
        }
    }
}

impl JsonType for bool {
    type Target = JsonTypeBoolMarker;

    fn to_json(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}

impl JsonType for serde_json::Value {
    type Target = JsonTypeObjectMarker;

    fn to_json(&self) -> Value {
        self.clone()
    }

    fn from_json(value: Value) -> Option<Self> {
        Some(value)
    }
}

impl<T> JsonType for Option<T>
where
    T: JsonType,
{
    type Target = T::Target;

    fn to_json(&self) -> Value {
        match self {
            Some(v) => v.to_json(),
            None => Value::Null,
        }
    }

    fn from_json(value: Value) -> Option<Self> {
        match value {
            Value::Null => Some(None),
            other => T::from_json(other).map(Some),
        }
    }
}
