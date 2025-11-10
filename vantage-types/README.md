# Vantage Types

A type system for implementing Persistence-specific type conversions.

## Problem

Serde does not enforce types. A `Decimal` can deserialize into a `String`, and a `Duration` can
deserialize into a number. This rules out Serde as a serialization framework when storing data into
a complex SQL database.

With many different databases around, each database client now must implement it's own type handling
system. Those are often hard to use and inconsistent. For `Vantage` to be capable having uniform
interraction with different databases - a type system should be robust and precise.

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
fn query(template: &str, parameters: Vec<AnyType3>) -> Result<Vec<IndexMap<AnyType3>>>;
```

AnyType3 is exported by an SDK and ensures type safety:

```rust
// AnyType3 can store either String or Email
let field_value = AnyType3::new(String::form("Hello, World!"));

// Back to string:
let hello: String = field_value.try_get().unwrap();

// This would fail, because type is important!
let hello_fail: Email = field_value.try_into;
```

## Typed record example

When loading data from database, use of incorrect fields can cause hidden issues. `vantage-types`
implements `#[persistence]` attribute macro, enabling type-safe serialization and deserialization of
record data:

```rust
#[persistence(Type3)]
struct User {
    name: String,
    email: Email,
}

let user = User {
    name: "John Doe".to_string(),
    email: Email::new("john", "example.com"),
};

// Convert to type-erased format for generic processing:
let values: IndexMap<String, AnyType3> = user.to_type3_map();

// Restore back when reading from database:
let restored = User::from_type3_map(values).unwrap();
```

Now if user attempts to load data a record that has incompatible types:

```rust
// Incorrect type definition
struct BadUser {
    name: String,
    email: String
}
let user_fail = BadUser::from_type3_map(values); // Fails, email is not String
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
- `MyTypeVariants` enum for runtime type identification. We will have to implement
  MyTypeVariants::from_json() for type detection.
- `AnyMyType` wrapper for type erasure
- `MyTypePersistence` trait for struct mapping
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
            ciborium::Value::Tag(6, _) => None, // Null values bypass variant check
            _ => None,
        }
    }
}
```

This approach allows nullable fields in structs while maintaining type safety and proper
serialization.

## Automatic Struct Persistence

The `#[persistence]` attribute generates automatic mapping between structs and type-erased storage:

```rust
#[derive(Debug, PartialEq)]
#[persistence(Type3)]
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
let storage_map = doc.to_type3_map();

// Each field is stored as AnyType3 with proper type information
assert_eq!(storage_map.get("title").unwrap().type_variant(), Some(Type3Variants::String));
assert_eq!(storage_map.get("subtitle").unwrap().type_variant(), Some(Type3Variants::String));
assert_eq!(storage_map.get("author").unwrap().type_variant(), Some(Type3Variants::Email));

// Test with None subtitle
let doc_no_subtitle = Document {
    title: "Another Article".to_string(),
    subtitle: None,
    author: Email::new("author", "blog.com"),
    published: false,
};

let storage_none = doc_no_subtitle.to_type3_map();
assert_eq!(storage_none.get("subtitle").unwrap().type_variant(), None); // None values have no variant

// Perfect round-trip conversion
let restored = Document::from_type3_map(storage_map).unwrap();
assert_eq!(doc, restored);
```

## Cross-Database Type Systems

Different persistence backends can use different value types while maintaining the same API:

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

// Same struct, dual persistence support
#[derive(Debug, PartialEq, Clone)]
#[persistence(SurrealType)]
#[persistence(PostgresType)]
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
let surreal_storage = user.to_surrealtype_map();
let postgres_storage = user.to_postgrestype_map();

// Both can be restored perfectly
let from_surreal = User::from_surrealtype_map(surreal_storage).unwrap();
let from_postgres = User::from_postgrestype_map(postgres_storage).unwrap();
```

## Integration with Vantage Framework

Vantage Types provides the foundation for type handling across the framework:

- **vantage-table**: Uses type systems for column definitions and value storage
- **vantage-expressions**: Leverages type abstraction for cross-database queries
- **vantage-surrealdb**: Implements SurrealDB-specific type variants and conversions
- **vantage-mongodb**: Uses BSON-compatible type systems for document storage

This unified approach enables applications to work seamlessly across different databases while
maintaining type safety and automatic conversions.
