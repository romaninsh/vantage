# Vantage-SurrealDB 0.3 Upgrade Plan

This document outlines the comprehensive upgrade plan for migrating vantage-surrealdb from the
current state to full 0.3 compatibility with the new Vantage architecture.

## Current State Analysis

### ✅ What's Already Working

- **surreal-client**: Low-level SurrealDB client is complete and functional
- **Basic SurrealDB struct**: Core wrapper around SurrealClient exists
- **SurrealColumn**: Column implementation with type support
- **Query builders**: SurrealSelect and SurrealInsert partially implemented
- **Expression system**: Basic expr! macro and TypedExpression working
- **Thing/Identifier**: SurrealDB-specific types implemented

### ❌ What Needs Upgrading

- **DataSource trait**: Not implemented for SurrealDB
- **TableSource trait**: Partially implemented but not 0.3 compatible
- **Type system integration**: Not using vantage-types properly
- **Value handling**: Mix of CBOR/JSON, inconsistent Record<T> usage
- **Query execution**: Direct execution vs trait-based execution
- **Table extensions**: SurrealTableExt not integrated with 0.3 patterns

## Phase 1: Core DataSource Implementation

### Step 1.1: Implement DataSource Trait

**File**: `src/surrealdb/datasource.rs` (new) **Dependencies**: surreal-client, vantage-expressions

```rust
use vantage_expressions::traits::datasource::{DataSource, ExprDataSource, SelectableDataSource};

impl DataSource for SurrealDB {}

impl ExprDataSource<AnySurrealType> for SurrealDB {
    async fn execute(&self, expr: &Expression<AnySurrealType>) -> Result<AnySurrealType>;
    fn defer(&self, expr: Expression<AnySurrealType>) -> DeferredFn<AnySurrealType>;
}

impl SelectableDataSource<AnySurrealType> for SurrealDB {
    type Select = SurrealSelect;
    fn select(&self) -> Self::Select;
    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<AnySurrealType>>;
}
```

**Tasks**:

- [ ] Move current execute logic from SurrealDB to ExprDataSource::execute
- [ ] Implement proper parameter binding using AnySurrealType
- [ ] Create defer implementation for lazy query execution
- [ ] Integrate SurrealSelect with SelectableDataSource

### Step 1.2: Fix Value Type System

**Files**: `src/surrealdb.rs`, all query builders **Dependencies**: vantage-types, surreal-client
types

**Current Issues**:

- Mixing `serde_json::Value` and `AnySurrealType`
- Direct CBOR conversion instead of using vantage-types
- No `Record<AnySurrealType>` usage

**Tasks**:

- [ ] Standardize on `AnySurrealType` as primary value type
- [ ] Replace manual CBOR conversion with vantage-types conversions
- [ ] Update all query builders to work with `Expression<AnySurrealType>`
- [ ] Implement proper `Record<AnySurrealType>` handling

## Phase 2: Query Builder Modernization

### Step 2.1: Update SurrealSelect

**File**: `src/select/mod.rs` **Dependencies**: vantage-expressions new traits

**Current Issues**:

- Uses old Selectable trait patterns
- Manual query building instead of Expression-based
- No proper result type handling

**Tasks**:

- [ ] Implement `Selectable<AnySurrealType>` properly
- [ ] Add result type generics: `SurrealSelect<ResultType>`
- [ ] Support `result::Rows`, `result::SingleRow`, `result::List`, `result::Single`
- [ ] Integrate with new Expression system from vantage-expressions
- [ ] Remove manual SQL string building

### Step 2.2: Update SurrealInsert

**File**: `src/insert/mod.rs` **Dependencies**: vantage-dataset traits

**Tasks**:

- [ ] Align with InsertableValueSet patterns
- [ ] Support both ID-based and auto-ID inserts
- [ ] Use Record<AnySurrealType> for data
- [ ] Integration with entity conversion

### Step 2.3: Create SurrealUpdate/SurrealDelete

**Files**: `src/update/mod.rs`, `src/delete/mod.rs` (new)

**Tasks**:

- [ ] Create update query builder following SurrealSelect patterns
- [ ] Create delete query builder
- [ ] Implement WritableValueSet support
- [ ] Support conditional updates/deletes

## Phase 3: TableSource Implementation

### Step 3.1: Complete TableSource Trait

**File**: `src/surrealdb/tablesource.rs` **Reference**:
`vantage-table/src/mocks/mock_table_source.rs`

**Current Issues**:

- Partially implemented with old trait signatures
- Missing many required methods
- No integration with new Column system

**Tasks**:

- [ ] Update trait signature to match current TableSource definition
- [ ] Fix all method signatures to use `Record<AnySurrealType>`
- [ ] Implement all missing methods:
  - [ ] `list_table_values`
  - [ ] `get_table_value`
  - [ ] `get_table_some_value`
  - [ ] `insert_table_value`
  - [ ] `replace_table_value`
  - [ ] `patch_table_value`
  - [ ] `delete_table_value`
  - [ ] `delete_table_all_values`
  - [ ] `insert_table_return_id_value`
  - [ ] `get_count`
  - [ ] `get_sum`

### Step 3.2: Column System Integration

**File**: `src/surrealdb/tablesource.rs`

**Tasks**:

- [ ] Update `create_column` to return `SurrealColumn<Type>`
- [ ] Implement `to_any_column` and `from_any_column`
- [ ] Fix `search_expression` to use new TableLike interface
- [ ] Support proper SurrealDB column types (Any, String, Integer, etc.)

### Step 3.3: Entity Integration

**Files**: All table operation files

**Tasks**:

- [ ] Update all methods to work with `Entity<AnySurrealType>`
- [ ] Support automatic entity serialization/deserialization
- [ ] Handle SurrealDB-specific entity patterns (Thing IDs, embedded docs)

## Phase 4: Table Extensions

### Step 4.1: Update SurrealTableExt

**File**: `src/table/ext.rs` **Reference**: Current `SurrealTableCore` patterns

**Tasks**:

- [ ] Remove old extension methods that conflict with 0.3
- [ ] Keep SurrealDB-specific extensions:
  - [ ] `.with_id(thing_id)` for Thing ID conditions
  - [ ] `.select_surreal()` for SurrealDB-specific select
  - [ ] `.select_surreal_first()`, `.select_surreal_column()` etc.
- [ ] Ensure compatibility with new Table<SurrealDB, E> structure
- [ ] Update all methods to use AssociatedExpression patterns

### Step 4.2: AnyTable Integration

**File**: `src/any.rs` (new) **Dependencies**: vantage-table AnyTable

**Tasks**:

- [ ] Ensure SurrealDB tables work with AnyTable
- [ ] Test downcasting from AnyTable to `Table<SurrealDB, E>`
- [ ] Verify JSON value operations work correctly
- [ ] Add SurrealDB-specific AnyTable helper methods if needed

## Phase 5: Testing & Examples

### Step 5.1: Update Unit Tests

**Files**: All test files

**Tasks**:

- [ ] Update all tests to use new trait implementations
- [ ] Replace direct SurrealDB calls with trait-based calls
- [ ] Add tests for TableSource implementation
- [ ] Test AnyTable integration
- [ ] Add tests for cross-database query scenarios

### Step 5.2: Update Examples

**File**: `examples/`

**Tasks**:

- [ ] Update basic connection example
- [ ] Add Table<SurrealDB, Entity> usage example
- [ ] Add AnyTable usage example
- [ ] Add cross-database query example
- [ ] Update README.md with 0.3 API

### Step 5.3: Integration Tests

**Files**: `tests/integration/`

**Tasks**:

- [ ] Test with bakery_model3 integration
- [ ] Test vantage-config dynamic table creation
- [ ] Test relationship traversal with AnyTable
- [ ] Performance benchmarks vs 0.2

## Phase 6: Documentation & Migration

### Step 6.1: API Documentation

**Files**: All public modules

**Tasks**:

- [ ] Add comprehensive doc comments
- [ ] Document migration path from 0.2
- [ ] Add examples for common patterns
- [ ] Document SurrealDB-specific features

### Step 6.2: Migration Guide

**File**: `MIGRATION.md` (new)

**Tasks**:

- [ ] Document breaking changes from 0.2
- [ ] Provide code examples for common migration scenarios
- [ ] List deprecated APIs and their replacements
- [ ] Performance considerations and optimizations

## Dependencies & Prerequisites

### Required Crate Updates

- ✅ surreal-client: Already compatible
- ✅ vantage-core: Error handling ready
- ✅ vantage-expressions: Expression system ready
- ✅ vantage-types: Entity/Record system ready
- ✅ vantage-table: TableSource traits ready
- ✅ vantage-dataset: ValueSet traits ready

### External Dependencies

- SurrealDB server for testing
- Update Cargo.toml dependencies to latest versions
- Ensure feature flags are properly configured

## Success Criteria

### ✅ Phase 1 Complete When:

- [ ] `SurrealDB` implements all DataSource traits
- [ ] Value type system is consistent (AnySurrealType everywhere)
- [ ] Basic query execution works through traits

### ✅ Phase 2 Complete When:

- [ ] All query builders implement correct traits
- [ ] Expression system integration works
- [ ] Result type handling is correct

### ✅ Phase 3 Complete When:

- [ ] `Table<SurrealDB, E>` works with all vantage-table operations
- [ ] All TableSource methods implemented and tested
- [ ] Column system fully integrated

### ✅ Phase 4 Complete When:

- [ ] SurrealDB-specific extensions work with 0.3 API
- [ ] AnyTable integration tested and working
- [ ] No API conflicts with core vantage-table

### ✅ Phase 5 Complete When:

- [ ] All tests pass with new implementation
- [ ] Examples demonstrate 0.3 patterns
- [ ] Integration with other vantage crates verified

### ✅ Phase 6 Complete When:

- [ ] Documentation is complete and accurate
- [ ] Migration path is clearly documented
- [ ] Performance is comparable or better than 0.2

## Timeline Estimate

- **Phase 1**: 2-3 days (Core DataSource traits)
- **Phase 2**: 3-4 days (Query builders update)
- **Phase 3**: 4-5 days (TableSource implementation)
- **Phase 4**: 2-3 days (Table extensions)
- **Phase 5**: 3-4 days (Testing & examples)
- **Phase 6**: 1-2 days (Documentation)

**Total**: ~15-20 days of focused development

## Risk Assessment

### High Risk

- **Complex type conversions**: AnySurrealType ↔ Record<AnySurrealType> ↔ Entity
- **CBOR/JSON impedance mismatch**: Need careful handling of SurrealDB's CBOR protocol
- **Query parameter binding**: SurrealDB's parameter system vs vantage expressions

### Medium Risk

- **Performance regression**: New abstraction layers might impact performance
- **Breaking API changes**: Existing code using vantage-surrealdb will need updates
- **Test coverage**: Ensuring all SurrealDB features work through new traits

### Low Risk

- **Documentation**: Straightforward once implementation is complete
- **Examples**: Can be updated incrementally
- **Integration**: Other vantage crates designed to work with this pattern

## Notes

- This plan focuses on making vantage-surrealdb fully compatible with 0.3 architecture
- The existing codebase provides a good foundation but needs significant refactoring
- Priority should be on getting core traits working before advanced features
- Consider creating feature flags to maintain backward compatibility during transition
