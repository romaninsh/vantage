# Vantage Types

A type system for implementing Persistence-specific type conversions.

## Problem

Serde does not enforce types. A `Decimal` can deserialize into a `String`, and a `Duration` can
deserialize into a number. This rules out Serde as a serialization framework when storing data into
a complex SQL database.

With many different databases around, each database client now must implement its own type handling
system. Those are often hard to use and inconsistent. For `Vantage` to be capable of having uniform
interaction with different databases - a type system should be robust and precise.

`vantage-types` implements exactly this kind of type system:

- Easily implement type system in your Database Rust SDK, including all your types.
- Allow users of your Database SDK to implement type conversions.
- `vantage-types` provides automatic mechanism for type erasure (`AnyType`)
- `vantage-types` provides a macro for type-safe serialization/deserialization into a particular
  type system.

## Basic example - single field value

Lets imagine a persistence, which can only store 2 types: String and Email. Both are stored using
binary CBOR format. A database SDK would need to add this:

```rust
use vantage_types::{vantage_type_system, persistence};

vantage_type_system! {
    type_trait: Type3,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Email]
}

// Implement those manually or with blanket impls
impl Type3 for String {..}
impl Type3 for Email {..}
```

Now SDK can use type-erased arguments to preserve type for both query parameters as well as return
type.

```rust
fn query(template: &str, parameters: Vec<AnyType3>) -> Result<Vec<IndexMap<String, AnyType3>>>;
```

AnyType3 is exported by an SDK and ensures type safety:

```rust
// AnyType3 can store either String or Email
let field_value = AnyType3::new(String::from("Hello, World!"));

// Back to string:
let hello: String = field_value.try_get().unwrap();

// This would fail, because type is important!
let hello_fail: Option<Email> = field_value.try_get::<Email>(); // Returns None
```

## Typed record example

When loading data from database, use of incorrect fields can cause hidden issues. `vantage-types`
implements `#[entity]` attribute macro, enabling type-safe serialization and deserialization of
record data:

```rust
#[entity(Type3)]
struct User {
    name: String,
    email: Email,
}

let user = User {
    name: "John Doe".to_string(),
    email: Email::new("john", "example.com"),
};

// Convert to type-erased format for generic processing:
let values: Record<AnyType3> = user.into_record();

// Restore back when reading from database:
let restored = User::from_record(values).unwrap();
```

Now if user attempts to load data a record that has incompatible types:

```rust
// Incorrect type definition
struct BadUser {
    name: String,
    email: String
}
let user_fail = BadUser::from_record(values); // Fails, email field type mismatch
```

## Type System Generation

The `vantage_type_system!` macro creates a complete type system with automatic conversions:

```rust
vantage_type_system! {
    type_trait: MyType,           // Main trait name
    method_name: json,            // Serialization method prefix
    value_type: serde_json::Value, // Underlying value type
    type_variants: [Int, Float, Decimal]
}
```

This generates:

- `MyType` trait with `to_json()` and `from_json()` methods
- `MyTypeVariants` enum for runtime type identification. You must implement
  `MyTypeVariants::from_json()` for type detection.
- `AnyMyType` wrapper for type erasure
- Type marker structs for compile-time safety

## Custom Type Implementation

Implement the generated trait for your types:

```rust
use rust_decimal::Decimal;

// Custom Decimal implementation for high-precision values
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

// Override the macro-generated variant detection with custom logic
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
```

## Optional Values

Handle nullable fields by implementing a custom Null type and Option<T> support:

```rust
// Define system with Null type for optionals
vantage_type_system! {
    type_trait: Type3,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Email, Null]
}

// Custom Null type for representing null values
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Null;

impl Type3 for Null {
    type Target = Type3NullMarker;

    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Tag(6, Box::new(ciborium::Value::Null))
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(6, _) => Some(Null),
            _ => None,
        }
    }
}

// Implement Option<String> support
impl Type3 for Option<String> {
    type Target = Type3StringMarker;

    fn to_cbor(&self) -> ciborium::Value {
        match self {
            Some(s) => ciborium::Value::Text(s.clone()),
            None => ciborium::Value::Tag(6, Box::new(ciborium::Value::Null)),
        }
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(6, _) => Some(None),
            ciborium::Value::Text(s) => Some(Some(s)),
            _ => None,
        }
    }
}

// Update variant detection to handle nulls
impl Type3Variants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(Type3Variants::String),
            ciborium::Value::Tag(1000, _) => Some(Type3Variants::Email),
            ciborium::Value::Tag(6, _) => Some(Type3Variants::Null),
            _ => None,
        }
    }
}
```

This approach allows nullable fields in structs while maintaining type safety and proper
serialization.

## Entity macro

The `#[entity]` generates implements Entity trait for your structure, which otherwise would require
you to convert all fields into/from Record<AnyType3>. Record relies on IndexMap and preserves field
order.

```rust
#[derive(Debug, PartialEq)]
#[entity(Type3)]
struct Document {
    title: String,
    subtitle: Option<String>,
    author: Email,
    published: bool,
}

let doc = Document {
    title: "My Article".to_string(),
    subtitle: Some("A comprehensive guide".to_string()),
    author: Email::new("author", "blog.com"),
    published: true,
};

// Automatic conversion to storage format
let storage_record: Record<AnyType3> = doc.into_record();

// Each field is stored as AnyType3 with proper type information
assert_eq!(storage_record.get("title").unwrap().type_variant(), Some(Type3Variants::String));
assert_eq!(storage_record.get("subtitle").unwrap().type_variant(), Some(Type3Variants::String));
assert_eq!(storage_record.get("author").unwrap().type_variant(), Some(Type3Variants::Email));

// Test with None subtitle
let doc_no_subtitle = Document {
    title: "Another Article".to_string(),
    subtitle: None,
    author: Email::new("author", "blog.com"),
    published: false,
};

let storage_none: Record<AnyType3> = doc_no_subtitle.into_record();
assert_eq!(storage_none.get("subtitle").unwrap().type_variant(), None); // None values have no variant

// Perfect round-trip conversion
let restored = Document::from_record(storage_record).unwrap();
assert_eq!(doc, restored);
```

## Serde Integration

If you enable `serde` feature, vantage-types will automatically implement
`Entity<serde_json::Value>` if your structure implements `Serialize` and `Deserialize`. This means
no extra boiler plate for any persistence that uses JSON as underlying format.

## Cross-Database Type Systems

Multiple type systems can be defined and Entity can be implemented several types - for different
value types:

```rust
// For SurrealDB with CBOR
vantage_type_system! {
    type_trait: SurrealType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Decimal, RId]
}

// For PostgreSQL with JSON
vantage_type_system! {
    type_trait: PostgresType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [String, Decimal, Uuid]
}

// Same struct, dual Entity support
#[derive(Debug, PartialEq, Clone)]
#[entity(SurrealType)]
#[entity(PostgresType)]
struct User {
    name: String,
    balance: Decimal,
}

// Different storage formats for the same data:
// SurrealDB: Decimal stored as Tag(200, "1234.56")
// PostgreSQL: Decimal stored as {"decimal": "1234.56"}

let user = User {
    name: "John Doe".to_string(),
    balance: Decimal::from_str_exact("1234.56").unwrap(),
};

// Store to both formats
let surreal_storage: Record<AnySurrealType> = user.clone().into_record();
let postgres_storage: Record<AnyPostgresType> = user.clone().into_record();

// Both can be restored perfectly
let from_surreal = User::from_record(surreal_storage).unwrap();
let from_postgres = User::from_record(postgres_storage).unwrap();
```

## Integration with Vantage Framework

Vantage Types provides the foundation for type handling across the framework:

- **vantage-table**: Uses type systems for column definitions and value storage
- **vantage-expressions**: Leverages type abstraction for cross-database queries
- **vantage-surrealdb**: Implements SurrealDB-specific type variants and conversions
- **vantage-mongodb**: Uses BSON-compatible type systems for document storage

This unified approach enables applications to work seamlessly across different databases while
maintaining type safety and automatic conversions.

## Implementing Persistence Engines with Record<T>

When building a persistence engine (like CSV, SurrealDB, or MongoDB adapters), `Record<T>` provides
a standardized interface for handling both structured and unstructured data. Vantage allows you to
implement a "glue" between underlying implementation and universal interface the rest of Vantage
ecosystem can use.

### Implementing CSV file handling - Vantage-way

To make CSV a valid persistence in Vantage - you would need to implement traits such as
`InsertableValueSet` and `ReadableValueSet`. Lets go through implementation scenario.

1. Creating a clear type system, that your persistent supports.
2. Implement type conversions for well-known types.
3. Allow users to implement Type conversions for additional types.
4. Implement methods required by traits from `vantage-dataset` / `vantage-table` crates.

In example below, we will implement two methods:

- `read_csv_contents() -> Result<Vec<Record<AnyCsvType>>>`
- `insert_csv_record(record: impl IntoRecord<AnyCsvType>>) -> Result<()>`

Additionally - those methods will use `Result` from `vantage-core` for uniform error handling.

### CSV Type System

CSV file only works with text, so we define a single type_variant: Text and use `String` as the
underlying value type.

```rust
// Define your persistence-specific type system
vantage_type_system! {
    type_trait: CsvType,
    method_name: csv_string,
    value_type: String,
    type_variants: [Text]
}

impl CsvType for String {
    type Target = CsvTypeTextMarker;

    fn to_csv_string(&self) -> String {
        self.clone()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        Some(value)
    }
}
```

If your CSV file implementation has some specific convention for storing other types - you can
implement CsvType for those types:

```rust
impl CsvType for Date {
    type Target = CsvTypeTextMarker;

    fn to_csv_string(&self) -> String {
        self.format("%Y-%m-%d").to_string()
    }

    fn from_csv_string(value: String) -> Option<Self> {
        Date::parse_from_str(&value, "%Y-%m-%d").ok()
    }
}
```

User of your persistence may also implement CsvType for their own types:

```rust
impl CsvType for Email {
    type Target = CsvTypeTextMarker;

    fn to_csv_string(&self) -> String {
        format!("{}@{}", self.local_part, self.domain)
    }

    fn from_csv_string(value: String) -> Option<Self> {
        let parts: Vec<&str> = value.split('@').collect();
        if parts.len() == 2 {
            Some(Email {
                local_part: parts[0].to_string(),
                domain: parts[1].to_string(),
            })
        } else {
            None
        }
    }
}
```

All types implementing `CsvType` trait will become first-class citizens - with consistent storage
and retrieval behavior. We are using single-variant "Text" - because of soft boundaries between
types, but your persistence logic can be more complex if needed.

### Implementing interaction with records

Your interface will need to work with Records and Entities (user-defined structs). You can use
struct `Record<CsvType>` and traits `IntoRecord<CsvType>` / `TryFromRecord<CsvType>` for this.

```rust
async fn actually_read_csv_contents() -> Result<Vec<IndexMap<String, String>>, std::io::Error>;
async fn actually_insert_csv_record(data: IndexMap<String, String>) -> Result<(), std::io::Error>;
```

Lets wrap those methods for vantage:

```rust
use vantage_core::{Result, Context};
use vantage_types::prelude::*;

async fn read_csv_contents() -> Result<Vec<Record<AnyCsvType>>> {
    // convert error into VantageError
    let contents = actually_read_csv_contents().await.context("Failed to read CSV contents")?;

    // Convert each row into Record<AnyCsvType>
    Ok(contents.into_iter().map(|row| {
        let record = Record::from_indexmap(row); // Record<String>
        Record::<AnyCsvType>::try_from_record(&record).unwrap() // convert to Record<AnyCsvType>
    }).collect())
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
    actually_insert_csv_record(indexmap).await.context("Failed to insert CSV record")
}
```

### Using Entities

If your crate uses serde_json::Value as underlying value type - enable serde feature in
`vantage-types` crate. However, if you use a more nuanced type system - users can use
`#[entity(YourType)]` macro to automatically implement `IntoRecord<AnyCsvType>` and
`TryFromRecord<AnyCsvType>` for their structs.

```rust
#[entity(CsvType)]
struct User {
    name: String,
    email: Email,
}

insert_csv_record(User {
    name: "John Doe".to_string(),
    email: Email::new("john", "example.com"),
}).await?;


let records = read_csv_contents().await?;
for record in records {

    let user: User = User::try_from_record(&record).unwrap();
    println!("User: {} ({})", user.name, user.email);

    // alternatively:
    println!("User: {} ({})", record["name"].value(), record["email"].value());
}
```

This can further be abstracted by type generics - that's what `vantage-table` does quite extensively
by using explicit entity implementations.
