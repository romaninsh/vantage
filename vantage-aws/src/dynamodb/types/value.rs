//! `AnyDynamoType` extras: untyped constructor, `From` impls, `Expressive`.

use super::{AnyDynamoType, AttributeValue, DynamoType, DynamoTypeLMarker, DynamoTypeNullMarker};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyDynamoType {
    /// Build a value with no variant tag — used for AttributeValues
    /// arriving from DynamoDB, where we trust the wire shape and let
    /// `try_get` attempt the read without enforcing a tag.
    pub fn untyped(value: AttributeValue) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// `AnyDynamoType` is itself a `DynamoType` — passthrough.
impl DynamoType for AnyDynamoType {
    type Target = DynamoTypeNullMarker;

    fn to_attr(&self) -> AttributeValue {
        self.value().clone()
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        AnyDynamoType::from_attr(&value)
    }
}

/// `Vec<AnyDynamoType>` — used by `column_table_values_expr` to project
/// a column's values across rows.
impl DynamoType for Vec<AnyDynamoType> {
    type Target = DynamoTypeLMarker;

    fn to_attr(&self) -> AttributeValue {
        AttributeValue::L(self.iter().map(|v| v.value().clone()).collect())
    }

    fn from_attr(value: AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::L(arr) => Some(arr.into_iter().map(AnyDynamoType::untyped).collect()),
            _ => None,
        }
    }
}

macro_rules! impl_from_for_any_dynamo {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyDynamoType {
                fn from(val: $ty) -> Self {
                    AnyDynamoType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any_dynamo!(i32, i64, f64, bool, String, Vec<u8>);

impl From<&str> for AnyDynamoType {
    fn from(val: &str) -> Self {
        AnyDynamoType::new(val.to_string())
    }
}

impl From<crate::dynamodb::id::DynamoId> for AnyDynamoType {
    fn from(val: crate::dynamodb::id::DynamoId) -> Self {
        AnyDynamoType::new(val.into_string())
    }
}

// Expressive impls — pass scalars directly into dynamo expressions.
macro_rules! impl_expressive_for_dynamo_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyDynamoType> for $ty {
                fn expr(&self) -> Expression<AnyDynamoType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyDynamoType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_dynamo_scalar!(i32, i64, f64, bool, String);

impl Expressive<AnyDynamoType> for &str {
    fn expr(&self) -> Expression<AnyDynamoType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyDynamoType::from(*self))],
        )
    }
}

impl Expressive<AnyDynamoType> for AnyDynamoType {
    fn expr(&self) -> Expression<AnyDynamoType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}

// ── AnyTable / refs bridge ────────────────────────────────────────────
//
// `with_one`/`with_many` and `AnyTable::from_table()` require
// `T::Value: Into<ciborium::Value> + From<ciborium::Value>` and the
// `serde_json::Value` equivalents. We do these as **shape-natural**
// conversions: a CBOR `Text` becomes `AttributeValue::S`, an `Integer`
// becomes `N`, etc. This is what user-supplied JSON in CLI/UI layers
// looks like; round-tripping through tagged wire form (`{"S": "x"}`)
// would force every caller to know DynamoDB's serialization tags.

impl From<AnyDynamoType> for ciborium::Value {
    fn from(val: AnyDynamoType) -> Self {
        attr_to_cbor(val.into_value())
    }
}

impl From<ciborium::Value> for AnyDynamoType {
    fn from(val: ciborium::Value) -> Self {
        AnyDynamoType::untyped(cbor_to_attr(val))
    }
}

impl From<AnyDynamoType> for serde_json::Value {
    fn from(val: AnyDynamoType) -> Self {
        attr_to_plain_json(val.into_value())
    }
}

impl From<serde_json::Value> for AnyDynamoType {
    fn from(val: serde_json::Value) -> Self {
        AnyDynamoType::untyped(plain_json_to_attr(val))
    }
}

fn attr_to_cbor(av: AttributeValue) -> ciborium::Value {
    use ciborium::Value as Cbor;
    match av {
        AttributeValue::S(s) => Cbor::Text(s),
        AttributeValue::N(ref n) => {
            if let Ok(i) = n.parse::<i64>() {
                Cbor::Integer(i.into())
            } else if let Ok(f) = n.parse::<f64>() {
                Cbor::Float(f)
            } else {
                Cbor::Text(n.clone())
            }
        }
        AttributeValue::B(b) => Cbor::Bytes(b),
        AttributeValue::Bool(b) => Cbor::Bool(b),
        AttributeValue::Null => Cbor::Null,
        AttributeValue::L(items) => Cbor::Array(items.into_iter().map(attr_to_cbor).collect()),
        AttributeValue::M(map) => Cbor::Map(
            map.into_iter()
                .map(|(k, v)| (Cbor::Text(k), attr_to_cbor(v)))
                .collect(),
        ),
        AttributeValue::SS(s) => Cbor::Array(s.into_iter().map(Cbor::Text).collect()),
        AttributeValue::NS(s) => Cbor::Array(s.into_iter().map(Cbor::Text).collect()),
        AttributeValue::BS(b) => Cbor::Array(b.into_iter().map(Cbor::Bytes).collect()),
    }
}

/// `serde_json::Value::serialize` writes Numbers as a 1-key map with this
/// magic key — a private newtype marker used to preserve the Number kind
/// through serde. CBOR captures it verbatim, so the round-trip
/// `serde_json → ciborium → AttributeValue` would otherwise produce an
/// `M`-shaped value with this nonsense key. Unwrap it back to a plain `N`.
const SERDE_JSON_NUMBER_MARKER: &str = "$serde_json::private::Number";

fn cbor_to_attr(val: ciborium::Value) -> AttributeValue {
    use ciborium::Value as Cbor;
    match val {
        Cbor::Text(s) => AttributeValue::S(s),
        Cbor::Integer(i) => AttributeValue::N(i128::from(i).to_string()),
        Cbor::Float(f) => AttributeValue::N(f.to_string()),
        Cbor::Bytes(b) => AttributeValue::B(b),
        Cbor::Bool(b) => AttributeValue::Bool(b),
        Cbor::Null => AttributeValue::Null,
        Cbor::Array(items) => AttributeValue::L(items.into_iter().map(cbor_to_attr).collect()),
        Cbor::Map(pairs) => {
            // Detect the `serde_json::Number` private marker and unwrap
            // it. Without this, every CLI-supplied integer would land as
            // an `M`-shaped attribute and DynamoDB would store garbage.
            if pairs.len() == 1
                && let Some((Cbor::Text(k), Cbor::Text(num))) = pairs.first()
                && k == SERDE_JSON_NUMBER_MARKER
            {
                return AttributeValue::N(num.clone());
            }
            let mut map = indexmap::IndexMap::new();
            for (k, v) in pairs {
                let key = match k {
                    Cbor::Text(s) => s,
                    other => format!("{:?}", other),
                };
                map.insert(key, cbor_to_attr(v));
            }
            AttributeValue::M(map)
        }
        Cbor::Tag(_, inner) => cbor_to_attr(*inner),
        _ => AttributeValue::Null,
    }
}

fn attr_to_plain_json(av: AttributeValue) -> serde_json::Value {
    use serde_json::Value as J;
    match av {
        AttributeValue::S(s) => J::String(s),
        AttributeValue::N(n) => n
            .parse::<i64>()
            .ok()
            .map(|i| J::Number(i.into()))
            .or_else(|| {
                n.parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map(J::Number)
            })
            .unwrap_or(J::String(n)),
        AttributeValue::B(b) => J::Array(b.into_iter().map(|byte| J::Number(byte.into())).collect()),
        AttributeValue::Bool(b) => J::Bool(b),
        AttributeValue::Null => J::Null,
        AttributeValue::L(items) => J::Array(items.into_iter().map(attr_to_plain_json).collect()),
        AttributeValue::M(map) => J::Object(
            map.into_iter()
                .map(|(k, v)| (k, attr_to_plain_json(v)))
                .collect(),
        ),
        AttributeValue::SS(s) => J::Array(s.into_iter().map(J::String).collect()),
        AttributeValue::NS(s) => J::Array(s.into_iter().map(J::String).collect()),
        AttributeValue::BS(_) => J::Null,
    }
}

fn plain_json_to_attr(val: serde_json::Value) -> AttributeValue {
    use serde_json::Value as J;
    match val {
        J::String(s) => AttributeValue::S(s),
        J::Number(n) => AttributeValue::N(n.to_string()),
        J::Bool(b) => AttributeValue::Bool(b),
        J::Null => AttributeValue::Null,
        J::Array(items) => AttributeValue::L(items.into_iter().map(plain_json_to_attr).collect()),
        J::Object(map) => AttributeValue::M(
            map.into_iter()
                .map(|(k, v)| (k, plain_json_to_attr(v)))
                .collect(),
        ),
    }
}
