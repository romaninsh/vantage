use vantage_types::vantage_type_system;

// CSV type system — values are stored as strings with type variant tags.
// Mirrors SurrealDB's pattern (`AnySurrealType` / `ciborium::Value`)
// but uses `String` as the underlying storage since CSV is text-based.
vantage_type_system! {
    type_trait: CsvType,
    method_name: csv_string,
    value_type: String,
    type_variants: [String, Int, Float, Bool, Json, Animal, List]
}

// Variant detection from raw CSV string (used when no column type is known)
impl CsvTypeVariants {
    pub fn from_csv_string(_value: &String) -> Option<Self> {
        // Without column type info, everything is a string
        Some(CsvTypeVariants::String)
    }
}

// --- Type implementations ---

impl CsvType for String {
    type Target = CsvTypeStringMarker;

    fn to_csv_string(&self) -> String {
        self.clone()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        Some(value)
    }
}

impl CsvType for i64 {
    type Target = CsvTypeIntMarker;

    fn to_csv_string(&self) -> String {
        self.to_string()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        value.parse().ok()
    }
}

impl CsvType for f64 {
    type Target = CsvTypeFloatMarker;

    fn to_csv_string(&self) -> String {
        self.to_string()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        value.parse().ok()
    }
}

impl CsvType for bool {
    type Target = CsvTypeBoolMarker;

    fn to_csv_string(&self) -> String {
        self.to_string()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        match value.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
}

impl CsvType for serde_json::Value {
    type Target = CsvTypeJsonMarker;

    fn to_csv_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        serde_json::from_str(&value).ok()
    }
}

impl<T> CsvType for Option<T>
where
    T: CsvType,
{
    type Target = T::Target;

    fn to_csv_string(&self) -> String {
        match self {
            Some(v) => v.to_csv_string(),
            None => String::new(),
        }
    }

    fn from_csv_string(value: String) -> Option<Self> {
        if value.is_empty() {
            Some(None)
        } else {
            T::from_csv_string(value).map(Some)
        }
    }
}

impl CsvType for Vec<AnyCsvType> {
    type Target = CsvTypeListMarker;

    fn to_csv_string(&self) -> String {
        // Encode each element as "variant_index\tvalue" separated by newlines
        self.iter()
            .map(|v| {
                let variant_idx = match v.type_variant() {
                    Some(CsvTypeVariants::String) => "0",
                    Some(CsvTypeVariants::Int) => "1",
                    Some(CsvTypeVariants::Float) => "2",
                    Some(CsvTypeVariants::Bool) => "3",
                    Some(CsvTypeVariants::Json) => "4",
                    Some(CsvTypeVariants::Animal) => "5",
                    Some(CsvTypeVariants::List) => "6",
                    None => "N",
                };
                format!("{}\t{}", variant_idx, v.value())
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn from_csv_string(value: String) -> Option<Self> {
        if value.is_empty() {
            return Some(Vec::new());
        }
        let items: Vec<AnyCsvType> = value
            .lines()
            .filter_map(|line| {
                let (variant_str, val) = line.split_once('\t')?;
                let variant = match variant_str {
                    "0" => Some(CsvTypeVariants::String),
                    "1" => Some(CsvTypeVariants::Int),
                    "2" => Some(CsvTypeVariants::Float),
                    "3" => Some(CsvTypeVariants::Bool),
                    "4" => Some(CsvTypeVariants::Json),
                    "5" => Some(CsvTypeVariants::Animal),
                    "6" => Some(CsvTypeVariants::List),
                    "N" => None,
                    _ => return None,
                };
                Some(parse_with_type(val, variant))
            })
            .collect();
        Some(items)
    }
}

// From impls for common types — enables Operation::eq() with native Rust types
impl From<i64> for AnyCsvType {
    fn from(v: i64) -> Self {
        AnyCsvType::new(v)
    }
}
impl From<f64> for AnyCsvType {
    fn from(v: f64) -> Self {
        AnyCsvType::new(v)
    }
}
impl From<bool> for AnyCsvType {
    fn from(v: bool) -> Self {
        AnyCsvType::new(v)
    }
}

// Expressive impls — allows passing scalars directly to Operation methods (eq, gt, etc.)
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

macro_rules! impl_expressive_for_csv_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyCsvType> for $ty {
                fn expr(&self) -> Expression<AnyCsvType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyCsvType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_csv_scalar!(i64, f64, bool, String);

impl Expressive<AnyCsvType> for &str {
    fn expr(&self) -> Expression<AnyCsvType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyCsvType::new(self.to_string()))],
        )
    }
}

impl Expressive<AnyCsvType> for AnyCsvType {
    fn expr(&self) -> Expression<AnyCsvType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}
impl From<serde_json::Value> for AnyCsvType {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => AnyCsvType {
                value: String::new(),
                type_variant: None,
            },
            serde_json::Value::Bool(b) => AnyCsvType::new(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    AnyCsvType::new(i)
                } else if let Some(f) = n.as_f64() {
                    AnyCsvType::new(f)
                } else {
                    AnyCsvType::new(n.to_string())
                }
            }
            serde_json::Value::String(s) => AnyCsvType::new(s),
            other => AnyCsvType::new(other.to_string()),
        }
    }
}

/// Parse a raw CSV string into an `AnyCsvType` using the column's type variant.
///
/// If `variant` is `None`, the value is stored as a plain string.
pub fn parse_with_type(raw: &str, variant: Option<CsvTypeVariants>) -> AnyCsvType {
    if raw.is_empty() {
        return AnyCsvType {
            value: String::new(),
            type_variant: None,
        };
    }

    match variant {
        Some(CsvTypeVariants::Int) => {
            if let Some(n) = i64::from_csv_string(raw.to_string()) {
                AnyCsvType::new(n)
            } else {
                // Parse failed — store as string
                AnyCsvType::new(raw.to_string())
            }
        }
        Some(CsvTypeVariants::Float) => {
            if let Some(n) = f64::from_csv_string(raw.to_string()) {
                AnyCsvType::new(n)
            } else {
                AnyCsvType::new(raw.to_string())
            }
        }
        Some(CsvTypeVariants::Bool) => {
            if let Some(b) = bool::from_csv_string(raw.to_string()) {
                AnyCsvType::new(b)
            } else {
                AnyCsvType::new(raw.to_string())
            }
        }
        Some(CsvTypeVariants::Json) => {
            if let Some(v) = serde_json::Value::from_csv_string(raw.to_string()) {
                AnyCsvType::new(v)
            } else {
                AnyCsvType::new(raw.to_string())
            }
        }
        // HACK: Animal stored as string with Animal variant tag.
        // TODO: Make extensible at compile time.
        Some(CsvTypeVariants::Animal) => AnyCsvType {
            value: raw.to_string(),
            type_variant: Some(CsvTypeVariants::Animal),
        },
        Some(CsvTypeVariants::List) => {
            // Lists are stored in encoded form; parse via CsvType impl
            if let Some(list) = Vec::<AnyCsvType>::from_csv_string(raw.to_string()) {
                AnyCsvType::new(list)
            } else {
                AnyCsvType::new(raw.to_string())
            }
        }
        Some(CsvTypeVariants::String) | None => AnyCsvType::new(raw.to_string()),
    }
}

/// Convert `AnyCsvType` to `serde_json::Value` for interop with JSON-based tools.
impl From<AnyCsvType> for serde_json::Value {
    fn from(csv: AnyCsvType) -> Self {
        match csv.type_variant() {
            Some(CsvTypeVariants::Int) => csv
                .try_get::<i64>()
                .map(|n| serde_json::Value::Number(n.into()))
                .unwrap_or(serde_json::Value::String(csv.into_value())),
            Some(CsvTypeVariants::Float) => csv
                .try_get::<f64>()
                .and_then(|n| serde_json::Number::from_f64(n).map(serde_json::Value::Number))
                .unwrap_or(serde_json::Value::String(csv.into_value())),
            Some(CsvTypeVariants::Bool) => csv
                .try_get::<bool>()
                .map(serde_json::Value::Bool)
                .unwrap_or(serde_json::Value::String(csv.into_value())),
            Some(CsvTypeVariants::Json) => csv
                .try_get::<serde_json::Value>()
                .unwrap_or(serde_json::Value::String(csv.into_value())),
            Some(CsvTypeVariants::List) => {
                let items = csv.try_get::<Vec<AnyCsvType>>().unwrap_or_default();
                serde_json::Value::Array(items.into_iter().map(serde_json::Value::from).collect())
            }
            Some(CsvTypeVariants::String) | Some(CsvTypeVariants::Animal) => {
                serde_json::Value::String(csv.into_value())
            }
            None => {
                if csv.value().is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(csv.into_value())
                }
            }
        }
    }
}

/// Convert `Record<AnyCsvType>` to `Record<serde_json::Value>`.
pub fn record_to_json(
    record: vantage_types::Record<AnyCsvType>,
) -> vantage_types::Record<serde_json::Value> {
    record
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::from(v)))
        .collect()
}

// -- CBOR bridge (for AnyTable interop) --------------------------------------
//
// `AnyTable` carries `Record<ciborium::Value>` across the type-erased
// boundary. CSV is a string-based format with no native CBOR
// representation; we round-trip via JSON so we get the same lossy
// behaviour as the JSON bridge above (binary → "[binary]" etc., handled
// by serde's CBOR↔JSON conversion).

impl From<ciborium::Value> for AnyCsvType {
    fn from(v: ciborium::Value) -> Self {
        let json: serde_json::Value =
            serde_json::to_value(&v).unwrap_or(serde_json::Value::Null);
        AnyCsvType::from(json)
    }
}

impl From<AnyCsvType> for ciborium::Value {
    fn from(csv: AnyCsvType) -> Self {
        let json = serde_json::Value::from(csv);
        ciborium::Value::serialized(&json).unwrap_or(ciborium::Value::Null)
    }
}

use vantage_types::TerminalRender;

impl TerminalRender for AnyCsvType {
    fn render(&self) -> String {
        match self.type_variant() {
            // HACK: Animal is domain-specific, hardcoded here for now.
            // TODO: Make custom type rendering extensible at compile time.
            Some(CsvTypeVariants::Animal) => match self.value().as_str() {
                "cat" => "🐱",
                "dog" => "🐶",
                "pig" => "🐷",
                "cow" => "🐮",
                "chicken" => "🐔",
                other => return other.to_string(),
            }
            .to_string(),
            None if self.value().is_empty() => "-".to_string(),
            _ => self.value().clone(),
        }
    }

    fn color_hint(&self) -> Option<&'static str> {
        match self.type_variant() {
            Some(CsvTypeVariants::Bool) => {
                if self.value() == "true" {
                    Some("green")
                } else {
                    Some("red")
                }
            }
            None if self.value().is_empty() => Some("dim"),
            _ => None,
        }
    }
}

/// Map a Rust type name (from `ColumnLike::get_type()`) to a `CsvTypeVariants`.
///
/// This is used to look up column types when parsing CSV rows.
pub fn variant_from_type_name(type_name: &str) -> Option<CsvTypeVariants> {
    // get_type() returns std::any::type_name which gives full paths
    match type_name {
        s if s.contains("i64") || s.contains("i32") || s.contains("u64") || s.contains("u32") => {
            Some(CsvTypeVariants::Int)
        }
        s if s.contains("f64") || s.contains("f32") => Some(CsvTypeVariants::Float),
        s if s.contains("bool") => Some(CsvTypeVariants::Bool),
        s if s.contains("Value") || s.contains("Json") => Some(CsvTypeVariants::Json),
        s if s.contains("String") || s.contains("str") => Some(CsvTypeVariants::String),
        // HACK: Animal is a domain-specific type hardcoded here for now.
        // TODO: Make custom type registration extensible at compile time.
        s if s.contains("Animal") => Some(CsvTypeVariants::Animal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_string() {
        let any = AnyCsvType::new("hello".to_string());
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::String));
        assert_eq!(any.try_get::<String>(), Some("hello".to_string()));
    }

    #[test]
    fn test_roundtrip_int() {
        let any = AnyCsvType::new(42_i64);
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::Int));
        assert_eq!(any.try_get::<i64>(), Some(42));
        assert_eq!(any.value(), "42");
    }

    #[test]
    fn test_roundtrip_float() {
        let any = AnyCsvType::new(2.72_f64);
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::Float));
        assert!(any.try_get::<f64>().is_some());
    }

    #[test]
    fn test_roundtrip_bool() {
        let any = AnyCsvType::new(true);
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::Bool));
        assert_eq!(any.try_get::<bool>(), Some(true));
        assert_eq!(any.value(), "true");
    }

    #[test]
    fn test_roundtrip_json() {
        let obj = serde_json::json!({"stock": 50});
        let any = AnyCsvType::new(obj.clone());
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::Json));
        assert_eq!(any.try_get::<serde_json::Value>(), Some(obj));
    }

    #[test]
    fn test_parse_with_type_int() {
        let any = parse_with_type("300", Some(CsvTypeVariants::Int));
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::Int));
        assert_eq!(any.try_get::<i64>(), Some(300));
    }

    #[test]
    fn test_parse_with_type_bool() {
        let any = parse_with_type("true", Some(CsvTypeVariants::Bool));
        assert_eq!(any.try_get::<bool>(), Some(true));
    }

    #[test]
    fn test_parse_with_type_json() {
        let any = parse_with_type(r#"{"stock":50}"#, Some(CsvTypeVariants::Json));
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::Json));
        let v = any.try_get::<serde_json::Value>().unwrap();
        assert_eq!(v["stock"], serde_json::json!(50));
    }

    #[test]
    fn test_parse_with_type_none_is_string() {
        let any = parse_with_type("42", None);
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::String));
        assert_eq!(any.try_get::<String>(), Some("42".to_string()));
    }

    #[test]
    fn test_parse_empty_is_null() {
        let any = parse_with_type("", Some(CsvTypeVariants::Int));
        assert_eq!(any.type_variant(), None);
    }

    #[test]
    fn test_variant_from_type_name_lookup() {
        assert_eq!(variant_from_type_name("i64"), Some(CsvTypeVariants::Int));
        assert_eq!(
            variant_from_type_name("alloc::string::String"),
            Some(CsvTypeVariants::String)
        );
        assert_eq!(variant_from_type_name("bool"), Some(CsvTypeVariants::Bool));
        assert_eq!(
            variant_from_type_name("serde_json::value::Value"),
            Some(CsvTypeVariants::Json)
        );
    }

    #[test]
    fn test_roundtrip_list() {
        let list = vec![
            AnyCsvType::new("hello".to_string()),
            AnyCsvType::new(42_i64),
            AnyCsvType::new(true),
        ];
        let any = AnyCsvType::new(list);
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::List));

        let recovered = any.try_get::<Vec<AnyCsvType>>().unwrap();
        assert_eq!(recovered.len(), 3);
        assert_eq!(recovered[0].try_get::<String>(), Some("hello".to_string()));
        assert_eq!(recovered[1].try_get::<i64>(), Some(42));
        assert_eq!(recovered[2].try_get::<bool>(), Some(true));
    }

    #[test]
    fn test_option_roundtrip() {
        let some_val: Option<i64> = Some(42);
        let any = AnyCsvType::new(some_val);
        assert_eq!(any.type_variant(), Some(CsvTypeVariants::Int));
        assert_eq!(any.try_get::<Option<i64>>(), Some(Some(42)));

        let none_val: Option<i64> = None;
        let any = AnyCsvType::new(none_val);
        assert_eq!(any.value(), "");
    }
}
