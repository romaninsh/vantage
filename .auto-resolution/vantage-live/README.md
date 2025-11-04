# vantage-live

Live data synchronization layer for the Vantage framework.

## Overview

`vantage-live` provides an in-memory caching layer with async backend persistence, designed for building responsive UIs that work with remote data sources. It bridges the gap between instant UI updates and eventual database consistency.

## Key Features

- **LiveTable**: Cache layer over any backend storage (SQL, SurrealDB, MongoDB, etc.)
- **RecordEdit**: Editing sessions with change tracking and snapshot management
- **Async persistence**: Write operations return immediately, persist in background
- **Conflict detection**: Track remote changes vs local edits
- **Field-level diffing**: Know exactly which fields were modified
- **No validation enforcement**: UI controls validation timing

## Architecture

```
UI Layer
   ↓
RecordEdit (editing session)
   ↓
LiveTable (synchronization)
   ↓
├─ Cache (fast: ImTable, ReDB, Redis)
└─ Backend (permanent: SurrealDB, PostgreSQL, MongoDB)
```

## Usage

### Initialize LiveTable

```rust
use vantage_live::LiveTable;
use vantage_dataset::im::ImTable;

// Create backend (e.g., SurrealDB table)
let backend = BakeryTable::surreal_table(client, "bakery");

// Create cache (in-memory)
let cache = ImTable::new(&im_ds, "bakery_cache");

// Initialize LiveTable (populates cache from backend)
let mut live_table = LiveTable::new(backend, cache).await?;
```

### Edit Existing Record

```rust
// Start editing session
let mut edit = live_table.edit_record("bakery:123").await?;

// Direct field access via Deref
edit.name = "New Bakery Name".to_string();
edit.profit_margin = 0.25;

// Check what changed
println!("Modified fields: {:?}", edit.get_modified_fields());

// Highlight in UI
for field in edit.get_modified_fields() {
    if edit.is_field_modified(&field) {
        highlight_field(&field);
    }
}

// Save when ready (async persist to backend)
match edit.save().await? {
    SaveResult::Saved => println!("Success!"),
    SaveResult::Error(e) => show_error(&e),
    SaveResult::PartialSave(fields) => {
        println!("Some fields didn't persist: {:?}", fields);
    }
    _ => {}
}
```

### Create New Record

```rust
// Create new record
let mut edit = live_table.new_record(Bakery {
    name: "Brand New Bakery".to_string(),
    profit_margin: 0.20,
    ..Default::default()
});

// Edit before saving
edit.name = "Even Better Name".to_string();

// Save (returns real ID from backend)
match edit.save().await? {
    SaveResult::Created(real_id) => {
        println!("Created with ID: {}", real_id);
        // edit.id() now returns real_id
    }
    SaveResult::Error(e) => show_error(&e),
    _ => {}
}
```

### Handle Remote Changes

```rust
// Register callback for remote updates
let live_table = LiveTable::new(backend, cache)
    .await?
    .on_remote_change(|id| {
        println!("Record {} changed remotely", id);
        ui.refresh(id);
    });

// When LIVE query notifies of change
live_table.on_backend_change("bakery:123").await?;

// If user has edit session open, refresh snapshot
if let Some(mut edit) = active_sessions.get_mut("bakery:123") {
    let conflicts = edit.refresh_snapshot().await?;
    if !conflicts.is_empty() {
        show_warning(&format!(
            "Remote changes conflict with your edits: {:?}",
            conflicts
        ));
    }
}
```

## RecordEdit Lifetime

`RecordEdit` borrows mutably from `LiveTable`, ensuring:
- No orphaned edit sessions
- Automatic cleanup on drop
- Simple `edit.save()` API (no need to pass edit back to table)

```rust
{
    let mut edit = live_table.edit_record("id").await?;
    edit.name = "Changed".to_string();
    edit.save().await?;
} // edit dropped, live_table available again
```

## Snapshot Management

Each `RecordEdit` maintains:
- `live_snapshot`: State when editing started
- `local`: Current edited state
- `snapshot_time`: When snapshot was taken

This enables:
- Field-level change detection
- Conflict resolution (local vs remote changes)
- Revert to original state

```rust
// Check snapshot age
let age = edit.snapshot_time().elapsed()?;
if age > Duration::from_secs(300) {
    println!("Snapshot is {} seconds old", age.as_secs());
}

// Compare local vs snapshot
println!("Original: {}", edit.live_snapshot().name);
println!("Current: {}", edit.local().name);

// Revert changes
edit.revert();
```

## DataSet Trait Integration

`LiveTable` implements all standard Vantage dataset traits:
- `ReadableDataSet<E>`
- `ReadableValueSet`
- `WritableDataSet<E>`
- `WritableValueSet`
- `InsertableDataSet<E>`

This makes it a drop-in replacement for regular tables in existing code.

## No Validation

`vantage-live` does not enforce validation. Example:
- User typing email: `"john@"` (incomplete, but valid local state)
- Form can validate before calling `save()`
- Backend validates on persist

This gives UI full control over validation timing and UX.

## Save Error Handling

If `save()` fails, UI can:
1. Show error message
2. Let user correct data
3. Call `save()` again
4. Or refresh snapshot if backend changed

```rust
loop {
    match edit.save().await {
        Ok(SaveResult::Saved) => break,
        Ok(SaveResult::Error(e)) => {
            if user_wants_to_retry(&e) {
                continue; // Try again
            } else {
                edit.refresh_snapshot().await?;
                break; // Give up, reload
            }
        }
        _ => break,
    }
}
```

## Implementation Status

### ✅ Implemented (Interfaces)
- LiveTable struct with RwValueSet trait
- RecordEdit with lifetime management
- DataSet trait implementations
- SaveResult enum

### ⏳ TODO
- Actual implementation (currently all methods are `todo!()`)
- Helper functions (extract_id, diff_fields)
- Background persist worker
- SurrealDB LIVE query integration
- Tests

## Contributing

This crate is part of the Vantage framework rewrite (v0.2 → v0.3).

## License

MIT
