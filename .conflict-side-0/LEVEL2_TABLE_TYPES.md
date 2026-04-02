# Level 2 Table Generic Type Relationships

## Core Generic Type Hierarchy

### Table<T, E>

- **T: TableSource** - Data storage backend (Postgres, SurrealDB, MongoDB, etc.)
- **E: Entity** - User-defined business object struct

### TableSource Associated Types

```rust
trait TableSource: DataSource + Clone + 'static {
    type Column: ColumnLike + Clone + 'static;  // Backend-specific column definition
    type Value: Clone + Send + Sync + 'static;  // Raw value type for expressions/conditions
    type Id: 'static;                           // Primary key type (i64, String, ObjectId, etc.)
}
```

## Type Dependency Chain

```
Table<T: TableSource, E: Entity>
├── T::Column (column definitions in IndexMap<String, T::Column>)
├── T::Id (primary keys - used in DataSet operations)
├── T::Value (expressions, conditions, sorting)
│   ├── conditions: IndexMap<i64, Expression<T::Value>>
│   └── order_by: IndexMap<i64, (Expression<T::Value>, SortDirection)>
└── E (entity structs for business logic)
```

## Current Type Erasure Status

### ✅ Implemented

- **AnyTable** - Type-erased `Table<T, E>` with downcasting
- **AnyColumn** - Type-erased `T::Column` with `ColumnLike` trait
- **AnyExpression** - Type-erased `Expression<T::Value>` (vantage-expressions)

### ❌ Missing Type Erasure

#### Critical Gap: Expression Storage

Current: `IndexMap<i64, Expression<T::Value>>` in Table struct Problem: Cannot store different
`T::Value` types in same collection

**Immediate Solution Needed:**

```rust
// Change Table struct from:
conditions: IndexMap<i64, Expression<T::Value>>,
order_by: IndexMap<i64, (Expression<T::Value>, SortDirection)>,

// To:
conditions: IndexMap<i64, AnyExpression>,
order_by: IndexMap<i64, (AnyExpression, SortDirection)>,
```

#### AnyId

```rust
pub struct AnyId {
    inner: Box<dyn IdLike>,
    type_id: TypeId,
    type_name: &'static str,
}

trait IdLike: Send + Sync + 'static {
    fn as_string(&self) -> String;
    fn from_string(s: &str) -> Result<Box<dyn IdLike>>;
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn IdLike>;
}
```

#### AnyValue

```rust
pub struct AnyValue {
    inner: Box<dyn ValueLike>,
    type_id: TypeId,
    type_name: &'static str,
}

trait ValueLike: Send + Sync + 'static {
    fn to_json(&self) -> Result<serde_json::Value>;
    fn from_json(json: serde_json::Value) -> Result<Box<dyn ValueLike>>;
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn ValueLike>;
}
```

#### AnyTableSource

```rust
pub struct AnyTableSource {
    inner: Box<dyn TableSourceLike>,
    column_type_id: TypeId,
    value_type_id: TypeId,
    id_type_id: TypeId,
}

trait TableSourceLike: Send + Sync + 'static {
    fn create_column(&self, name: &str, table: &dyn TableLike) -> Box<dyn ColumnLike>;
    fn create_expression(&self, template: String, params: Vec<AnyValue>) -> AnyExpression;
    fn parse_id(&self, s: &str) -> Result<AnyId>;
    fn as_any(&self) -> &dyn Any;
    fn clone_box(&self) -> Box<dyn TableSourceLike>;
}
```

## Real Implementation Examples

Based on actual codebase analysis:

### SurrealDB Implementation

```rust
impl TableSource for SurrealDB {
    type Column = SurrealColumn;
    type Value = AnySurrealType;  // From surreal-client
    type Id = String;             // SurrealDB uses string IDs
}
```

### ReDB Implementation

```rust
impl TableSource for Redb {
    type Column = RedbColumn;
    type Value = RedbExpression;  // Custom expression type
    type Id = ???;                // Need to check implementation
}
```

### Mock Implementation

```rust
impl TableSource for MockTableSource {
    type Column = Column;                    // Basic column
    type Value = serde_json::Value;          // JSON for compatibility
    type Id = String;                        // String keys
}
```

## Problem Areas Requiring Fundamental Changes

### 1. Table Struct Storage Types

Current issue in `table/base.rs`:

```rust
pub(super) conditions: IndexMap<i64, Expression<T::Value>>,
pub(super) order_by: IndexMap<i64, (Expression<T::Value>, SortDirection)>,
```

**Solution:** Change to use `AnyExpression`:

```rust
pub(super) conditions: IndexMap<i64, AnyExpression>,
pub(super) order_by: IndexMap<i64, (AnyExpression, SortDirection)>,
```

### 2. TableLike Trait Compatibility

Current `TableLike::add_condition` takes `Box<dyn Any>` but needs type safety:

```rust
fn add_condition(&mut self, condition: Box<dyn Any + Send + Sync>) -> Result<()>;
```

**Better approach:**

```rust
fn add_condition(&mut self, condition: AnyExpression) -> Result<()>;
```

### 3. Cross-Database Value Conversion

Different databases use different value types:

- SurrealDB: `AnySurrealType`
- Mock: `serde_json::Value`
- ReDB: `RedbExpression`

Need value conversion traits for cross-database operations.

### 4. Column Storage Consistency

Good news: Already using type erasure in TableLike:

```rust
fn columns(&self) -> Arc<IndexMap<String, Arc<dyn ColumnLike>>>;
```

## Implementation Priority

1. **Fix Expression Storage** - Change Table struct to use AnyExpression (breaking change)
2. **AnyValue** - Enable cross-database value compatibility
3. **AnyId** - Standardize ID handling across databases
4. **AnyTableSource** - Complete type erasure for dynamic table creation
5. **Value Conversion Traits** - Enable seamless cross-database operations

## Type Safety vs Flexibility Trade-offs

### With Full Type Erasure:

- ✅ Single `AnyTable` type works across all databases
- ✅ Dynamic table creation from config files
- ✅ Generic UI components for any table
- ✅ Cross-database query composition
- ⚠️ Runtime type checking replaces compile-time safety
- ⚠️ Performance overhead from boxing/dynamic dispatch
- ⚠️ More complex error handling for type mismatches

### Current State Benefits:

- ✅ Compile-time type safety within single database
- ✅ Zero-cost abstractions for database-specific operations
- ✅ Clear type relationships in generic parameters
- ❌ Cannot mix databases in same data structure
- ❌ No dynamic table creation
- ❌ UI components must be database-specific

## Fundamental Changes Required for AnyTable

### Current AnyTable Limitations

The existing `AnyTable` in `any.rs` has several architectural issues:

1. **Incomplete Type Erasure**: Only erases `TableSource` and `Entity`, but underlying `Table<T, E>`
   still contains non-erased types
2. **Storage Type Conflicts**: `Table.conditions` uses `Expression<T::Value>` which cannot be stored
   in type-erased form
3. **Trait Mismatch**: `TableLike` expects `Box<dyn Any>` for conditions but `Table` implementation
   needs `T::Value` expressions

### Required Changes to Table Storage

**File: `vantage-table/src/table/base.rs`**

```rust
// BEFORE (current - causes type erasure issues):
pub(super) conditions: IndexMap<i64, Expression<T::Value>>,
pub(super) order_by: IndexMap<i64, (Expression<T::Value>, SortDirection)>,
pub(super) columns: IndexMap<String, T::Column>,

// AFTER (needed for full type erasure):
pub(super) conditions: IndexMap<i64, AnyExpression>,
pub(super) order_by: IndexMap<i64, (AnyExpression, SortDirection)>,
pub(super) columns: IndexMap<String, Arc<dyn ColumnLike>>,
```

### New Required Any Types

**AnyValue** - Essential for cross-database value conversion:

```rust
pub struct AnyValue {
    inner: Box<dyn ValueLike>,
    type_id: TypeId,
}

trait ValueLike: Send + Sync + 'static {
    fn to_json(&self) -> Result<serde_json::Value>;
    fn from_json(json: &serde_json::Value) -> Result<Box<dyn ValueLike>> where Self: Sized;
    fn clone_box(&self) -> Box<dyn ValueLike>;
}
```

**AnyTableSource** - Enable dynamic table source creation:

```rust
pub struct AnyTableSource {
    inner: Box<dyn TableSourceLike>,
    value_type_id: TypeId,
    id_type_id: TypeId,
}

trait TableSourceLike: Send + Sync + 'static {
    fn create_column(&self, name: &str, table: &dyn TableLike) -> Arc<dyn ColumnLike>;
    fn create_any_expression(&self, template: String, params: Vec<AnyValue>) -> AnyExpression;
    fn clone_box(&self) -> Box<dyn TableSourceLike>;
}
```

### Migration Strategy

**Phase 1: Expression Storage**

- Change `Table` struct to use `AnyExpression`
- Update `TableLike::add_condition` to accept `AnyExpression`
- Modify condition-related methods in table implementations

**Phase 2: Column Storage**

- Change `Table.columns` from `IndexMap<String, T::Column>` to
  `IndexMap<String, Arc<dyn ColumnLike>>`
- Update column creation and access methods

**Phase 3: Value System**

- Implement `AnyValue` with conversion traits
- Add value factory methods to `TableSource`

**Phase 4: Complete Type Erasure**

- Implement `AnyTableSource`
- Enable fully dynamic table creation

### Breaking Changes Impact

1. **All Table<T, E> implementations** must be updated for new storage types
2. **TableSource trait** needs additional methods for Any type creation
3. **Condition handling code** throughout codebase needs updating
4. **Database-specific extensions** (like SurrealTableExt) need Any type support

### Benefits After Migration

- Single `AnyTable` type works across all databases without type parameters
- Dynamic table creation from YAML/JSON configuration
- Generic UI adapters work with any database
- Cross-database operations (join SurrealDB table with MongoDB collection)
- Runtime table composition and modification
