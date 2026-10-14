//! `AnyCsvType`: values stored as text with a type-variant tag.

use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: CsvType,
    method_name: csv_string,
    value_type: String,
    type_variants: [String, Int, Float, Bool, Json, Animal, List]
}

impl CsvTypeVariants {
    /// Without column type info, every raw value is a string.
    pub fn from_csv_string(_value: &String) -> Option<Self> {
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
        // One line per element: "variant_index\tvalue".
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

/// The value as a condition preview shows it: text quoted, anything else as
/// stored.
impl std::fmt::Display for AnyCsvType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.type_variant() {
            Some(CsvTypeVariants::String) => write!(f, "{:?}", self.value()),
            _ => f.write_str(self.value()),
        }
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
/// An empty string is untyped null. With no variant, or text that doesn't
/// parse as the variant, the value is stored as a plain string.
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

// CBOR has no CSV representation, so the bridge goes through JSON and is as
// lossy as the JSON one. The shared walker is total: tagged CBOR values
// keep their payload instead of collapsing to Null.

impl From<ciborium::Value> for AnyCsvType {
    fn from(v: ciborium::Value) -> Self {
        AnyCsvType::from(vantage_types::cbor_to_json(&vantage_types::PlainDialect, v))
    }
}

impl From<AnyCsvType> for ciborium::Value {
    fn from(csv: AnyCsvType) -> Self {
        vantage_types::json_to_cbor(serde_json::Value::from(csv))
    }
}

use vantage_types::{RichText, Style, TerminalRender};

impl TerminalRender for AnyCsvType {
    fn render(&self) -> RichText {
        match self.type_variant() {
            // HACK: Animal is domain-specific, hardcoded here for now.
            // TODO: Make custom type rendering extensible at compile time.
            Some(CsvTypeVariants::Animal) => {
                let glyph = match self.value().as_str() {
                    "cat" => "🐱",
                    "dog" => "🐶",
                    "pig" => "🐷",
                    "cow" => "🐮",
                    "chicken" => "🐔",
                    other => return RichText::plain(other.to_string()),
                };
                RichText::plain(glyph.to_string())
            }
            Some(CsvTypeVariants::Bool) => {
                if self.value() == "true" {
                    RichText::styled("true", Style::Success)
                } else {
                    RichText::styled("false", Style::Error)
                }
            }
            None if self.value().is_empty() => RichText::styled("—", Style::Muted),
            _ => RichText::plain(self.value().clone()),
        }
    }
}

/// Map a Rust type name (from `ColumnLike::get_type()`) to a `CsvTypeVariants`.
///
/// Used to look up column types when parsing CSV rows.
pub fn variant_from_type_name(type_name: &str) -> Option<CsvTypeVariants> {
    // `type_name` is a full path, so match on substrings.
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
