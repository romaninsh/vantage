# vantage-mongodb

A MongoDB query builder for the Vantage framework that generates MongoDB-style JSON queries.

## Features

- **MongoDB-style JSON queries**: Generates queries that look like native MongoDB commands
- **Type-safe query building**: Leverages Rust's type system for safe query construction
- **Comprehensive operator support**: Supports all common MongoDB operators ($gt, $lt, $in, $or, $regex, etc.)
- **Expression integration**: Works seamlessly with the vantage-expressions framework
- **Full CRUD operations**: Support for find, insert, update, delete, and count operations

## Usage

### Basic Find Query

```rust
use vantage_mongodb::{Document, MongoSelect};

let query = MongoSelect::from_collection("users");
// Generates: db.users.find({})
```

### Find with Filter

```rust
let query = MongoSelect::from_collection("users")
    .filter(Document::filter("status", "active"));
// Generates: db.users.find({"status": "active"})
```

### Find with Operators

```rust
let query = MongoSelect::from_collection("products")
    .filter(Document::gt("price", 100));
// Generates: db.products.find({"price": {"$gt": 100}})
```

### Complex Queries

```rust
let query = MongoSelect::from_collection("orders")
    .filter(
        Document::new()
            .insert("status", "pending")
            .and("total", Document::new().insert("$gte", 50))
            .and("created_at", Document::new()
                .insert("$gte", "2024-01-01")
                .insert("$lt", "2024-12-31"))
    )
    .project(Document::new()
        .insert("order_id", 1)
        .insert("customer", 1)
        .insert("total", 1))
    .sort_by(Document::new().insert("created_at", -1))
    .limit(100);
```

### Insert Operations

```rust
use vantage_mongodb::MongoInsert;

// Insert one document
let query = MongoInsert::new("users")
    .insert_one(Document::new()
        .insert("name", "John Doe")
        .insert("email", "john@example.com")
        .insert("age", 30));
// Generates: db.users.insertOne({...})

// Insert multiple documents
let query = MongoInsert::new("users")
    .insert_many(vec![
        Document::new().insert("name", "Alice").insert("email", "alice@example.com"),
        Document::new().insert("name", "Bob").insert("email", "bob@example.com"),
    ]);
// Generates: db.users.insertMany([{...}, {...}])
```

### Update Operations

```rust
use vantage_mongodb::MongoUpdate;

let query = MongoUpdate::new("users")
    .filter(Document::filter("status", "pending"))
    .set_update(Document::new().insert("$set",
        Document::new()
            .insert("status", "active")
            .insert("updated_at", "2024-01-01")));
// Generates: db.users.updateMany({"status": "pending"}, {"$set": {...}})
```

### Delete Operations

```rust
use vantage_mongodb::MongoDelete;

let query = MongoDelete::new("users")
    .filter(Document::filter("status", "inactive"));
// Generates: db.users.deleteMany({"status": "inactive"})
```

### Count Operations

```rust
use vantage_mongodb::MongoCount;

let query = MongoCount::new("users")
    .filter(Document::gt("age", 18));
// Generates: db.users.countDocuments({"age": {"$gt": 18}})
```

## Supported Operators

- **Comparison**: `$gt`, `$gte`, `$lt`, `$lte`, `$ne`, `$in`
- **Logical**: `$or`, `$and`
- **Element**: `$exists`
- **Evaluation**: `$regex`
- **Array**: `$in`

## Document Builder

The `Document` struct provides a fluent API for building MongoDB documents:

```rust
let doc = Document::new()
    .insert("name", "John")
    .insert("age", 30)
    .and("status", "active");
```

### Operator Methods

```rust
// Comparison operators
Document::gt("age", 18)           // {"age": {"$gt": 18}}
Document::gte("score", 90)        // {"score": {"$gte": 90}}
Document::lt("price", 100)        // {"price": {"$lt": 100}}
Document::lte("count", 50)        // {"count": {"$lte": 50}}
Document::ne("status", "deleted") // {"status": {"$ne": "deleted"}}

// Array operators
Document::in_array("category", vec!["books", "electronics"])
// {"category": {"$in": ["books", "electronics"]}}

// Logical operators
Document::or(vec![
    Document::filter("status", "active"),
    Document::filter("priority", "high")
])
// {"$or": [{"status": "active"}, {"priority": "high"}]}

// Element operators
Document::exists("email", true)   // {"email": {"$exists": true}}

// Evaluation operators
Document::regex("name", "^John")  // {"name": {"$regex": "^John"}}
```

## Field Naming

The `Field` struct handles MongoDB field name escaping:

```rust
use vantage_mongodb::Field;

let field = Field::new("user.name");  // Generates: "user.name"
let field = Field::new("$set");       // Generates: "$set"
```

Fields are automatically quoted when they contain special characters, start with `$`, or contain dots.

## Integration with Vantage

This crate integrates seamlessly with the vantage-expressions framework and implements the `Select` trait:

```rust
use vantage_expressions::{Expression, protocol::select::Select};

let query = MongoSelect::from_collection("users").filter(Document::filter("status", "active"));
let expr: Expression = query.into();
println!("{}", expr.preview());

// Use as Select trait
let mut select = MongoSelect::from_collection("users");
select.add_where_condition(Document::filter("status", "active").into());
select.set_limit(Some(10), Some(0));
```

## Examples

See the `examples/` directory for more comprehensive usage examples.

## License

MIT OR Apache-2.0
