# Vantage Framework Architecture

## Core Design: 4-Level Capability Hierarchy

Vantage provides a unified interface for data operations across different storage backends through a 4-level capability hierarchy:

### Level 1: Custom DataSource → DataSet

**Purpose**: Simple data storage with basic CRUD operations
**Examples**: CSV files, in-memory storage, message queues, JSON files
**Interface**: Custom struct implementing `ReadableDataSet<E>`, `InsertableDataSet<E>`, `WritableDataSet<E>`
**Key Feature**: Direct entity operations without table structure

### Level 2: Basic TableSource → Table + Columns

**Purpose**: Structured table operations with CRUD and column definitions
**Examples**: Key-value stores (ReDB), simple databases, NoSQL without query language
**Interface**: `TableSource` - creates `Table<DS, E>` with column support
**Key Feature**: Column definitions and basic table structure

### Level 3: Advanced RDBMS → Full Query Support

**Purpose**: Complete SQL-like capabilities with expressions, joins, aggregations
**Examples**: SurrealDB, PostgreSQL, MongoDB (with aggregation pipeline)
**Interface**: `TableSource + QuerySource<T> + SelectSource<T>`
**Key Feature**: SELECT queries and expression building

### Level 4: Vendor Extensions → Database-Specific Features

**Purpose**: Vendor-specific optimizations and unique database features
**Examples**: SurrealTableExt, PostgresTableExt, MongoTableExt
**Interface**: Extension traits providing database-specific methods
**Key Feature**: Access to native database capabilities beyond standard SQL

## Type System Challenge

### Three Type Layers

Vantage must support three different type representations for the same data:

1. **Vendor-Native Types**: `AnySurrealType`, `PostgresType`, `MongoValue`
   - Native database types with full feature support
   - Best performance and capability access
   - Type-specific operations (e.g., SurrealDB record references)

2. **JSON Compatibility Layer**: `serde_json::Value`
   - Universal cross-database interchange format
   - Enables data migration between different backends
   - Required for UI frameworks and generic operations

3. **User Entity Types**: `impl Entity`
   - Application-defined structs (User, Order, Product)
   - Type-safe business logic operations
   - Compile-time validation and IDE support

### Type Erasure Requirements

The framework must support both:

- **Compile-time type safety** for performance-critical code
- **Runtime type erasure** for generic UI components and dynamic operations

Key challenge: Method naming that supports all three type layers while maintaining dyn-safe traits.

## Implementation Matrix

| DataSource   | Level | Dataset Traits | TableSource | QuerySource | SelectSource | Vendor Ext | Type Support    |
| ------------ | ----- | -------------- | ----------- | ----------- | ------------ | ---------- | --------------- |
| ImDataSource | 1     | ✅ R/W/I       | ❌          | ❌          | ❌           | ❌         | JSON only       |
| CSV Files    | 1     | ✅ R           | ❌          | ❌          | ❌           | ❌         | JSON only       |
| ReDB         | 2     | ✅ R/W/I       | ✅          | ❌          | ❌           | ❌         | JSON + RedbExpr |
| SurrealDB    | 3     | ✅ R/W/I       | ✅          | ✅          | ✅           | ✅         | All three       |
| PostgreSQL   | 3     | ✅ R/W/I       | ✅          | ✅          | ✅           | ✅         | All three       |

## Dyn-Safe Design Requirements

### Core Traits Must Be Object-Safe

All foundational traits must support dynamic dispatch:

- `TableLike` - Type-erased table operations
- `ColumnLike` - Type-erased column operations
- `ExpressionLike` - Type-erased expression operations
- `DataSourceLike` - Type-erased datasource operations

### Method Naming Strategy

Methods need clear naming to distinguish type layers:

**Entity Operations**:

- `get_entities()` → `Vec<E>`
- `get_entity_by_id(id)` → `E`

**Native Type Operations**:

- `get_native_values()` → `Vec<DS::NativeType>`
- `get_native_by_id(id)` → `DS::NativeType`

**JSON Operations**:

- `get_json_values()` → `Vec<serde_json::Value>`
- `get_json_by_id(id)` → `serde_json::Value`

### Type Erasure Wrappers

- `AnyTable` - Type-erased table with downcasting support
- `AnyExpression` - Type-erased expression wrapper
- `AnyDataSource` - Type-erased datasource operations
- `AnyColumn` - Type-erased column operations

## Cross-Database Compatibility

All datasources provide three access patterns:

1. **Native Access**: Maximum performance, full features
2. **JSON Access**: Cross-database compatibility
3. **Entity Access**: Type-safe application integration

This enables:

- Generic UI components working with any datasource
- Data migration between different databases
- Performance optimization through native types when available
- Graceful fallback to JSON when native types unavailable

## Benefits

1. **Progressive Enhancement**: Start simple (Level 1), upgrade capabilities as needed
2. **Type Flexibility**: Choose optimal type representation for each use case
3. **Runtime Flexibility**: Type erasure for generic components
4. **Vendor Optimization**: Access database-specific features when available
5. **Cross-Database**: Uniform interface with backend-specific optimizations
