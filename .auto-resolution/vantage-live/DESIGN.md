# vantage-live Design Document

This document captures the design decisions from the brainstorming session that led to the creation of `vantage-live`.

## Problem Statement

When building UIs that interact with remote databases, we face a tension:
- **UI needs**: Instant updates, responsive editing, visual feedback
- **Backend reality**: Network latency, transaction overhead, potential conflicts

Existing ORMs load records, let you edit them, and save back. But this doesn't handle:
1. Showing users which fields they've modified
2. Detecting when remote data changed while editing
3. Providing instant UI feedback while persisting async
4. Managing validation lifecycle (incomplete vs invalid states)

## Solution: LiveTable + RecordEdit

### Core Concepts

**LiveTable**: Synchronization layer between fast cache and permanent backend
- Cache: In-memory (ImTable), local disk (ReDB), or shared (Redis)
- Backend: Any database (SurrealDB, PostgreSQL, MongoDB)
- Implements all DataSet traits - drop-in replacement for regular tables

**RecordEdit**: Editing session that borrows from LiveTable
- Tracks local modifications vs live snapshot
- Provides field-level change detection
- Handles save lifecycle with conflict detection
- Lifetime-bound to LiveTable (can't outlive parent)

## Key Design Decisions

### 1. Snapshot-Based Change Tracking

Each `RecordEdit` maintains three states:
- `live_snapshot`: State when editing started (with timestamp)
- `local`: Current edited state
- Difference = modified fields

**Why?**
- Form fields can highlight what changed
- Detect conflicts: local edits vs remote changes
- Revert functionality without backend roundtrip
- Pre-validation can show "unsaved changes" indicator

### 2. No Internal State Machine

We considered tracking states like `Synced`, `Editing`, `Saving`, `Conflict`, `Error`.

**Decision**: Don't maintain persistent state. Instead:
- `save()` returns `SaveResult` enum immediately
- UI handles errors via result pattern
- Conflicts resolved by `refresh_snapshot()` returning conflict list
- No dragging state across operations

**Why?**
- Simpler mental model
- UI has full control over error handling
- No ambiguous "stuck" states
- Failed saves don't pollute object state

### 3. Manual Flush (No Auto-Save)

Unlike some ORMs, we don't auto-persist on field changes or timer.

**Decision**: User explicitly calls `edit.save()`

**Why?**
- Maps naturally to "Save" button in UI
- User controls when validation happens
- Batch edits before persist
- No surprise network calls

### 4. Validation Lives in UI Layer

`RecordEdit` allows invalid state (e.g., incomplete email while typing).

**Decision**: No validation in vantage-live

**Why?**
- Different UIs have different validation UX
- Pre-save validation vs post-save validation
- Backend is final validator anyway
- More flexible for progressive forms

### 5. RwValueSet (Not Entity-Based)

Backend and cache use `RwValueSet` (reads/writes JSON values) not typed entities.

**Decision**: Use trait objects `Box<dyn RwValueSet>`

**Why?**
- Avoids dragging generic parameters everywhere
- Cache can be different type than backend
- Supports `patch_id()` for partial updates
- More flexible than `WritableDataSet<E>`

### 6. RecordEdit Lifetime Binding

`RecordEdit<'a, E>` borrows `&'a mut LiveTable<E>`

**Decision**: Enforce lifetime, not independent struct

**Why?**
- Can't outlive LiveTable
- Automatic cleanup on drop
- Simple `edit.save()` API (no passing back to table)
- Mirrors existing `Record<'a, E, T>` pattern

### 7. New Record Handling

New records don't have backend ID yet.

**Decision**: Generate temp ID (`temp_{uuid}`), replace on save

**Flow**:
1. `new_record()` → `RecordEdit` with temp ID
2. User edits local state
3. `save()` → backend.insert() → returns real ID
4. Update `edit.id`, `live_snapshot`, `local` with persisted state
5. `SaveResult::Created(real_id)`

**Why?**
- UI can reference record before persist
- Uniform API for new/existing records
- ID transition handled transparently

### 8. Conflict Resolution Strategy

When remote data changes while editing:

**Decision**: No automatic resolution, provide tools

**Flow**:
1. `on_backend_change(id)` updates cache
2. If user has `RecordEdit` open, call `refresh_snapshot()`
3. Returns list of conflicting fields (edited locally AND changed remotely)
4. UI decides: keep local, overwrite, or merge

**Why?**
- Different apps need different strategies
- Some conflicts are benign (different fields)
- User context matters (which change is more important)
- Explicit > implicit

### 9. Partial Save Detection

After save, we verify persisted state matches local state.

**Decision**: Fetch fresh from backend, compare modified fields

**Flow**:
1. `save()` → backend.replace_id()
2. Fetch fresh: `backend.get_id(id)`
3. Compare: did our changed fields persist correctly?
4. If mismatch → `SaveResult::PartialSave(mismatched_fields)`
5. Update snapshot but keep local (user can retry)

**Why?**
- Database triggers might modify values
- Validation might coerce values
- Concurrent updates might win
- User needs to know what actually saved

## Architecture Layers

```
┌─────────────────────────────────────┐
│         UI Layer (GPUI)             │
│  - Forms, tables, dialogs           │
│  - Validation logic                 │
│  - Error handling                   │
└─────────────┬───────────────────────┘
              │
              ▼
┌─────────────────────────────────────┐
│      RecordEdit<'a, E>              │
│  - Local state                      │
│  - Snapshot management              │
│  - Change tracking                  │
│  - Save lifecycle                   │
└─────────────┬───────────────────────┘
              │
              ▼
┌─────────────────────────────────────┐
│       LiveTable<E>                  │
│  - Sync coordination                │
│  - Cache population                 │
│  - Remote change handling           │
│  - DataSet trait impl               │
└─────────────┬───────────────────────┘
              │
      ┌───────┴───────┐
      ▼               ▼
┌─────────┐     ┌──────────┐
│  Cache  │     │ Backend  │
│ (fast)  │     │(permanent)│
└─────────┘     └──────────┘
  ImTable        SurrealDB
  ReDB           PostgreSQL
  Redis          MongoDB
```

## Usage Patterns

### Pattern 1: Simple Edit Form

```rust
// Open form
let mut edit = live_table.edit_record("id").await?;

// Bind fields
form.bind("name", &mut edit.name);
form.bind("email", &mut edit.email);

// Highlight modified
for field in edit.get_modified_fields() {
    form.highlight(field);
}

// Save button
if save_clicked {
    match edit.save().await? {
        SaveResult::Saved => close_form(),
        SaveResult::Error(e) => show_error(e),
        _ => {}
    }
}
```

### Pattern 2: Multi-Field Form with Validation

```rust
let mut edit = live_table.edit_record("id").await?;

// User edits (validation deferred)
edit.email = user_input; // Might be incomplete

// Pre-save validation
if let Err(e) = validate(&edit) {
    show_validation_errors(e);
    return;
}

// Save
edit.save().await?;
```

### Pattern 3: Conflict Handling

```rust
// LIVE query callback
live_table.on_backend_change("id").await?;

// User has form open
if let Some(mut edit) = active_edits.get_mut("id") {
    let conflicts = edit.refresh_snapshot().await?;

    if !conflicts.is_empty() {
        let dialog = ConflictDialog::new(
            edit.live_snapshot(), // Remote version
            edit.local(),         // Local version
            conflicts,            // Conflicting fields
        );

        match dialog.show() {
            Resolution::KeepLocal => { /* continue editing */ }
            Resolution::UseRemote => edit.revert(),
            Resolution::Merge(merged) => *edit.local_mut() = merged,
        }
    }
}
```

### Pattern 4: Retry on Error

```rust
loop {
    match edit.save().await {
        Ok(SaveResult::Saved) => break,
        Ok(SaveResult::PartialSave(fields)) => {
            show_warning(&format!("These didn't save: {:?}", fields));
            if !user_wants_retry() { break; }
        }
        Ok(SaveResult::Error(e)) => {
            show_error(&e);
            if !user_wants_retry() { break; }
        }
        _ => break,
    }
}
```

## Implementation Notes

### Phase 1: Core Functionality
- [ ] `LiveTable::new()` - populate cache from backend
- [ ] `edit_record()` / `new_record()` - create RecordEdit
- [ ] `RecordEdit::save()` - persist with verification
- [ ] `get_modified_fields()` - JSON diffing
- [ ] `refresh_snapshot()` - conflict detection

### Phase 2: DataSet Traits
- [ ] Implement ReadableDataSet (proxy to cache)
- [ ] Implement WritableDataSet (update cache + backend)
- [ ] Implement InsertableDataSet (temp ID handling)
- [ ] Implement ValueSet traits

### Phase 3: Advanced Features
- [ ] Background persist queue (for write batching)
- [ ] SurrealDB LIVE query integration
- [ ] Event stream abstraction (LIVE, polling, Kafka)
- [ ] Cache eviction policies (LRU, TTL)

### Phase 4: Optimizations
- [ ] Patch vs replace (partial updates)
- [ ] Field-level subscriptions (only notify on specific changes)
- [ ] Optimistic locking (version tokens)
- [ ] Batch operations

## Future Considerations

### Multi-Table Transactions
If editing spans multiple tables (e.g., Order + OrderLineItems):
- Wrap in transaction context?
- Coordinate saves across LiveTables?
- Rollback on partial failure?

### Offline Support
If backend unreachable:
- Queue writes in cache
- Replay when connection restored
- Handle conflicts on reconnect

### Collaborative Editing
Multiple users editing same record:
- Operational transforms?
- CRDT-based merging?
- Lock acquisition?

## Open Questions

1. **Cache eviction**: Should LiveTable auto-evict stale records, or always hold full dataset?
2. **Patch support**: Should we detect backend PATCH capability and use it?
3. **Version tokens**: Should we track ETags/version numbers for optimistic locking?
4. **Background workers**: Should saves happen in separate task, or inline in `save()`?

## Related Work

- **ActiveRecord (Rails)**: Tracks dirty attributes, but no snapshot comparison
- **Hibernate (Java)**: Session-based, complex state management
- **Django ORM**: Simple but no conflict detection
- **Realm (Mobile)**: Live objects, but tightly coupled to storage

**Our approach**: Simpler than Hibernate, more powerful than Django, flexible like Realm but database-agnostic.

## Conclusion

`vantage-live` provides a thin, focused layer for synchronizing in-memory state with remote databases. By keeping concerns separated (validation in UI, state in RecordEdit, persistence in LiveTable), we create a flexible foundation for responsive data-driven applications.

The snapshot-based approach gives UIs the information they need (what changed, when, conflicts) without dictating how to handle it. Manual save gives users control. Lifetime binding keeps APIs simple.

Next step: Implement the `todo!()` methods and validate design with real-world usage in bakery_model3.
