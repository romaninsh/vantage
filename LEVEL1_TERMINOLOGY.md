# Level 1 DataSet Terminology

## Core Concepts

**DataSet**: A collection of data records in a datasource that can be accessed and manipulated through various operations.

**Record**: Representation of a single data record that can potentially be saved back to the datasource.

**Entity**: User-defined struct (User, Order, Product) that represents business objects.

**Value**: Raw `serde_json::Value` for cross-database compatibility.

## Set Type Classifications

### DataSet

- **Purpose**: Typed entity operations
- **Generic**: `<E: Entity>`
- **Returns**: Application-defined structs

### ValueSet

- **Purpose**: JSON operations for cross-database compatibility
- **Generic**: None (always `serde_json::Value`)
- **Returns**: Raw JSON values

### RecordSet

- **Purpose**: Record operations with ID tracking and save capability
- **Generic**: `<E: Entity>`
- **Returns**: `Record<E>` wrappers with `.save()` method

## Operation Type Matrix

| Set Type      | Readable | Insertable | Writable | Notes                |
| ------------- | -------- | ---------- | -------- | -------------------- |
| **DataSet**   | ✅       | ✅         | ✅       | Full entity CRUD     |
| **ValueSet**  | ✅       | ❌         | ✅       | JSON-only operations |
| **RecordSet** | ✅       | ❌         | ✅       | ID-tracked entities  |

## Trait Implementation Matrix

### ReadableDataSet<E>

```rust
async fn list(&self) -> Result<IndexMap<Self::Id, E>>
async fn get(&self, id: &Self::Id) -> Result<E>
async fn get_some(&self) -> Result<Option<(Self::Id, E)>>
```

### ReadableValueSet

```rust
async fn list_values(&self) -> Result<IndexMap<Self::Id, serde_json::Value>>
async fn get_value(&self, id: &Self::Id) -> Result<serde_json::Value>
async fn get_some_value(&self) -> Result<Option<(Self::Id, serde_json::Value)>>
```

### ReadableAsDataSet

```rust
async fn list_as<T: Entity>(&self) -> Result<IndexMap<Self::Id, T>>
async fn get_as<T: Entity>(&self, id: &Self::Id) -> Result<T>
async fn get_some_as<T: Entity>(&self) -> Result<Option<(Self::Id, T)>>
```

### InsertableDataSet<E>

```rust
async fn insert(&self, id: &Self::Id, record: E) -> Result<()>
```

### InsertableValueSet

```rust
async fn insert_value(&self, id: &Self::Id, record: serde_json::Value) -> Result<()>
```

### InsertableRecordSet<E>

```rust
async fn insert_record(&self, id: &Self::Id, record: E) -> Result<Record<'_, Self, E>>
```

### WritableDataSet<E>

```rust
async fn insert_id(&self, id: &Self::Id, record: E) -> Result<()>
async fn replace_id(&self, id: &Self::Id, record: E) -> Result<()>
async fn update<F>(&self, callback: F) -> Result<()>
```

### WritableValueSet

```rust
async fn insert_id_value(&self, id: &Self::Id, record: serde_json::Value) -> Result<()>
async fn replace_id_value(&self, id: &Self::Id, record: serde_json::Value) -> Result<()>
async fn patch_id(&self, id: &Self::Id, partial: serde_json::Value) -> Result<()>
async fn delete_id(&self, id: &Self::Id) -> Result<()>
async fn delete_all(&self) -> Result<()>
```

### RecordDataSet<E>

```rust
async fn get_record(&self, id: &Self::Id) -> Result<Option<Record<'_, Self, E>>>
```

### Importable<E>

```rust
async fn import<D: ReadableDataSet<E>>(&mut self, source: D) -> Result<()>
```
