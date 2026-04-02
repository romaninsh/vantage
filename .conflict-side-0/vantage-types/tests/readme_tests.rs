use indexmap::IndexMap;
use rust_decimal::Decimal;

use vantage_types::vantage_type_system;
use vantage_types_entity::entity;

// Basic example - single field value
vantage_type_system! {
    type_trait: Type3,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Email]
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Email {
    pub name: String,
    pub domain: String,
}

impl Email {
    pub fn new(name: &str, domain: &str) -> Self {
        Self {
            name: name.to_string(),
            domain: domain.to_string(),
        }
    }
}

impl std::fmt::Display for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.name, self.domain)
    }
}

impl Type3 for String {
    type Target = Type3StringMarker;

    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Text(self.clone())
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl Type3 for Email {
    type Target = Type3EmailMarker;

    fn to_cbor(&self) -> ciborium::value::Value {
        let array = vec![
            ciborium::value::Value::Text(self.name.clone()),
            ciborium::value::Value::Text(self.domain.clone()),
        ];
        ciborium::value::Value::Tag(1000, Box::new(ciborium::value::Value::Array(array)))
    }

    fn from_cbor(cbor: ciborium::value::Value) -> Option<Self> {
        let ciborium::value::Value::Tag(1000, boxed_value) = cbor else {
            return None;
        };
        let ciborium::value::Value::Array(arr) = boxed_value.as_ref() else {
            return None;
        };
        let name = match arr.first()? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };
        let domain = match arr.get(1)? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };
        Some(Email::new(name, domain))
    }
}

impl Type3Variants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(Type3Variants::String),
            ciborium::Value::Tag(1000, _) => Some(Type3Variants::Email),
            _ => None,
        }
    }
}

// Typed record example
#[derive(Debug, PartialEq, Clone)]
#[entity(Type3)]
struct User {
    name: String,
    email: Email,
}

// Type System Generation example
vantage_type_system! {
    type_trait: MyType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [Int, Float, Decimal]
}

impl MyType for i32 {
    type Target = MyTypeIntMarker;

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Number((*self).into())
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Number(n) if n.is_i64() => Some(n.as_i64()? as i32),
            _ => None,
        }
    }
}

impl MyType for f64 {
    type Target = MyTypeFloatMarker;

    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Number(
            serde_json::Number::from_f64(*self).unwrap_or_else(|| serde_json::Number::from(0)),
        )
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Number(n) if n.is_f64() => n.as_f64(),
            _ => None,
        }
    }
}

impl MyType for Decimal {
    type Target = MyTypeDecimalMarker;

    fn to_json(&self) -> serde_json::Value {
        // Store decimal as {"decimal": "decimal_string"} to avoid precision loss
        serde_json::json!({"decimal": self.to_string()})
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Object(obj) => {
                if let Some(serde_json::Value::String(decimal_str)) = obj.get("decimal") {
                    decimal_str.parse().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl MyTypeVariants {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Number(n) if n.is_i64() => Some(MyTypeVariants::Int),
            serde_json::Value::Number(n) if n.is_f64() => Some(MyTypeVariants::Float),
            serde_json::Value::Object(obj) => {
                if obj.contains_key("decimal") {
                    Some(MyTypeVariants::Decimal)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// Optional Values example
vantage_type_system! {
    type_trait: OptionalType,
    method_name: cbor_opt,
    value_type: ciborium::Value,
    type_variants: [String, Email, Null]
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Null;

impl OptionalType for Null {
    type Target = OptionalTypeNullMarker;

    fn to_cbor_opt(&self) -> ciborium::Value {
        ciborium::Value::Tag(6, Box::new(ciborium::Value::Null))
    }

    fn from_cbor_opt(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(6, _) => Some(Null),
            _ => None,
        }
    }
}

impl OptionalType for String {
    type Target = OptionalTypeStringMarker;

    fn to_cbor_opt(&self) -> ciborium::Value {
        ciborium::Value::Text(self.clone())
    }

    fn from_cbor_opt(value: ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl OptionalType for Email {
    type Target = OptionalTypeEmailMarker;

    fn to_cbor_opt(&self) -> ciborium::value::Value {
        let array = vec![
            ciborium::value::Value::Text(self.name.clone()),
            ciborium::value::Value::Text(self.domain.clone()),
        ];
        ciborium::value::Value::Tag(1000, Box::new(ciborium::value::Value::Array(array)))
    }

    fn from_cbor_opt(cbor: ciborium::value::Value) -> Option<Self> {
        let ciborium::value::Value::Tag(1000, boxed_value) = cbor else {
            return None;
        };
        let ciborium::value::Value::Array(arr) = boxed_value.as_ref() else {
            return None;
        };
        let name = match arr.first()? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };
        let domain = match arr.get(1)? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };
        Some(Email::new(name, domain))
    }
}

impl OptionalType for Option<String> {
    type Target = OptionalTypeNullMarker;

    fn to_cbor_opt(&self) -> ciborium::Value {
        match self {
            Some(s) => ciborium::Value::Text(s.clone()),
            None => ciborium::Value::Tag(6, Box::new(ciborium::Value::Null)),
        }
    }

    fn from_cbor_opt(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(6, _) => Some(None),
            ciborium::Value::Text(s) => Some(Some(s)),
            _ => None,
        }
    }
}

impl OptionalTypeVariants {
    pub fn from_cbor_opt(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(OptionalTypeVariants::String),
            ciborium::Value::Tag(1000, _) => Some(OptionalTypeVariants::Email),
            ciborium::Value::Tag(6, _) => None, // Null values bypass variant check
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
#[entity(OptionalType)]
struct Document {
    title: String,
    subtitle: Option<String>,
    author: Email,
    published: bool,
}

impl OptionalType for bool {
    type Target = OptionalTypeNullMarker; // Using Null marker for simplicity

    fn to_cbor_opt(&self) -> ciborium::Value {
        ciborium::Value::Bool(*self)
    }

    fn from_cbor_opt(value: ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Bool(b) => Some(b),
            _ => None,
        }
    }
}

// Cross-Database Type Systems
vantage_type_system! {
    type_trait: SurrealType,
    method_name: cbor_surreal,
    value_type: ciborium::Value,
    type_variants: [String, Decimal, RId]
}

vantage_type_system! {
    type_trait: PostgresType,
    method_name: json_pg,
    value_type: serde_json::Value,
    type_variants: [String, Decimal, Uuid]
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RId(String);

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Uuid(String);

impl SurrealType for String {
    type Target = SurrealTypeStringMarker;

    fn to_cbor_surreal(&self) -> ciborium::Value {
        ciborium::Value::Text(self.clone())
    }

    fn from_cbor_surreal(value: ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl SurrealType for Decimal {
    type Target = SurrealTypeDecimalMarker;

    fn to_cbor_surreal(&self) -> ciborium::Value {
        ciborium::Value::Tag(200, Box::new(ciborium::Value::Text(self.to_string())))
    }

    fn from_cbor_surreal(value: ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Tag(200, boxed) => {
                if let ciborium::Value::Text(s) = boxed.as_ref() {
                    s.parse().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl PostgresType for String {
    type Target = PostgresTypeStringMarker;

    fn to_json_pg(&self) -> serde_json::Value {
        serde_json::Value::String(self.clone())
    }

    fn from_json_pg(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl PostgresType for Decimal {
    type Target = PostgresTypeDecimalMarker;

    fn to_json_pg(&self) -> serde_json::Value {
        serde_json::json!({"decimal": self.to_string()})
    }

    fn from_json_pg(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Object(obj) => {
                if let Some(serde_json::Value::String(decimal_str)) = obj.get("decimal") {
                    decimal_str.parse().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SurrealTypeVariants {
    pub fn from_cbor_surreal(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(SurrealTypeVariants::String),
            ciborium::Value::Tag(200, _) => Some(SurrealTypeVariants::Decimal),
            _ => None,
        }
    }
}

impl PostgresTypeVariants {
    pub fn from_json_pg(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(_) => Some(PostgresTypeVariants::String),
            serde_json::Value::Object(obj) if obj.contains_key("decimal") => {
                Some(PostgresTypeVariants::Decimal)
            }
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
#[entity(SurrealType)]
#[entity(PostgresType)]
struct CrossDbUser {
    name: String,
    balance: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_types::{IntoRecord, TryFromRecord};

    #[test]
    fn test_basic_single_field_value() {
        // AnyType3 can store either String or Email
        let field_value = AnyType3::new(String::from("Hello, World!"));

        // Back to string:
        let hello: String = field_value.try_get().unwrap();
        assert_eq!(hello, "Hello, World!");

        // This would fail, because type is important!
        let hello_fail: Option<Email> = field_value.try_get::<Email>();
        assert!(hello_fail.is_none());
    }

    #[test]
    fn test_typed_record_example() {
        let user = User {
            name: "John Doe".to_string(),
            email: Email::new("john", "example.com"),
        };

        // Convert to type-erased format for generic processing:
        let values: vantage_types::Record<AnyType3> = user.clone().into_record();

        // Restore back when reading from database:
        let restored = User::from_record(values).unwrap();
        assert_eq!(user, restored);
    }

    #[test]
    fn test_custom_decimal_implementation() {
        let decimal = Decimal::from_str_exact("1234.5678").unwrap();
        let _any_decimal = AnyMyType::new(decimal);

        // Test serialization
        let json_value = decimal.to_json();
        let expected = serde_json::json!({"decimal": "1234.5678"});
        assert_eq!(json_value, expected);

        // Test deserialization
        let restored = Decimal::from_json(json_value).unwrap();
        assert_eq!(decimal, restored);

        // Test variant detection
        let variant = MyTypeVariants::from_json(&expected);
        assert_eq!(variant, Some(MyTypeVariants::Decimal));
    }

    #[test]
    fn test_optional_values() {
        let doc = Document {
            title: "My Article".to_string(),
            subtitle: Some("A comprehensive guide".to_string()),
            author: Email::new("author", "blog.com"),
            published: true,
        };

        // Automatic conversion to storage format
        let storage_record: vantage_types::Record<AnyOptionalType> = doc.clone().into_record();

        // Each field is stored as AnyOptionalType with proper type information
        assert!(storage_record.contains_key("title"));
        assert!(storage_record.contains_key("subtitle"));
        assert!(storage_record.contains_key("author"));
        assert!(storage_record.contains_key("published"));

        // Test with None subtitle
        let doc_no_subtitle = Document {
            title: "Another Article".to_string(),
            subtitle: None,
            author: Email::new("author", "blog.com"),
            published: false,
        };

        let storage_none: vantage_types::Record<AnyOptionalType> = doc_no_subtitle.into_record();
        // For None values, just check the field exists
        assert!(storage_none.contains_key("subtitle"));

        // Perfect round-trip conversion
        let restored = Document::from_record(storage_record).unwrap();
        assert_eq!(doc, restored);
    }

    #[test]
    fn test_cross_database_type_systems() {
        let user = CrossDbUser {
            name: "John Doe".to_string(),
            balance: Decimal::from_str_exact("1234.56").unwrap(),
        };

        // Store to both formats
        let surreal_storage: vantage_types::Record<AnySurrealType> = user.clone().into_record();
        let postgres_storage: vantage_types::Record<AnyPostgresType> = user.clone().into_record();

        // Both can be restored perfectly
        let from_surreal = CrossDbUser::from_record(surreal_storage).unwrap();
        let from_postgres = CrossDbUser::from_record(postgres_storage).unwrap();

        assert_eq!(user, from_surreal);
        assert_eq!(user, from_postgres);
    }

    #[tokio::test]
    async fn test_csv_examples() {
        // CSV persistence engine example
        vantage_type_system! {
            type_trait: CsvType,
            method_name: csv_string,
            value_type: String,
            type_variants: [Text]
        }
        use vantage_core::{Context, Result};
        use vantage_types::{IntoRecord, Record, TryFromRecord};

        impl CsvType for String {
            type Target = CsvTypeTextMarker;

            fn to_csv_string(&self) -> String {
                self.clone()
            }

            fn from_csv_string(value: String) -> Option<Self> {
                Some(value)
            }
        }

        impl CsvType for Email {
            type Target = CsvTypeTextMarker;

            fn to_csv_string(&self) -> String {
                format!("{}@{}", self.name, self.domain)
            }

            fn from_csv_string(value: String) -> Option<Self> {
                let parts: Vec<&str> = value.split('@').collect();
                if parts.len() == 2 {
                    Some(Email {
                        name: parts[0].to_string(),
                        domain: parts[1].to_string(),
                    })
                } else {
                    None
                }
            }
        }

        impl CsvTypeVariants {
            pub fn from_csv_string(_value: &String) -> Option<Self> {
                Some(CsvTypeVariants::Text)
            }
        }

        #[derive(Debug, PartialEq)]
        #[entity(CsvType)]
        struct User {
            name: String,
            email: Email,
        }
        // Layer 1: Low-level persistence operations (hardcoded for testing)
        async fn actually_read_csv_contents(
        ) -> std::result::Result<Vec<IndexMap<String, String>>, std::io::Error> {
            let mut result = Vec::new();

            let mut record1 = IndexMap::new();
            record1.insert("name".to_string(), "Alice".to_string());
            record1.insert("email".to_string(), "alice@paris.com".to_string());

            let mut record2 = IndexMap::new();
            record2.insert("name".to_string(), "Bob".to_string());
            record2.insert("email".to_string(), "bob@london.com".to_string());

            result.push(record1);
            result.push(record2);

            Ok(result)
        }

        async fn actually_insert_csv_record(
            _data: IndexMap<String, String>,
        ) -> std::result::Result<(), std::io::Error> {
            // In real implementation, this would write to CSV file
            Ok(())
        }

        // Layer 2: Vantage type system integration
        async fn read_csv_contents() -> Result<Vec<Record<AnyCsvType>>> {
            // convert error into VantageError
            let contents = actually_read_csv_contents()
                .await
                .context("Failed to read CSV contents")?;

            // Convert each row into Record<AnyCsvType>
            Ok(contents
                .into_iter()
                .map(|row| {
                    let record = Record::from_indexmap(row); // Record<String>
                    Record::<AnyCsvType>::try_from_record(&record).unwrap() // convert to Record<AnyCsvType>
                })
                .collect())
        }

        async fn insert_csv_record<T>(record: T) -> Result<()>
        where
            T: IntoRecord<AnyCsvType>,
        {
            let vantage_record = record.into_record();

            // Convert to underlying value type
            let string_record: Record<String> = vantage_record.into_record();

            // Convert Record<AnyCsvType> into IndexMap<String, String>
            let indexmap = string_record.into_inner();

            // Convert error into VantageError
            actually_insert_csv_record(indexmap)
                .await
                .context("Failed to insert CSV record")
        }

        let _ = insert_csv_record(User {
            name: "John Doe".to_string(),
            email: Email::new("john", "example.com"),
        })
        .await;

        let records = read_csv_contents().await.unwrap();
        for record in records {
            let user: User = User::try_from_record(&record).unwrap();
            println!("User: {} ({})", user.name, user.email);

            // alternatively:
            println!(
                "User: {} ({})",
                record["name"].value(),
                record["email"].value()
            );
        }
    }
}
