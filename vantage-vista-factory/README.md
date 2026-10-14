# Vantage Vista Factory

A name-to-`Vista` catalog that spans datasources, and the place where references between different
persistences are followed.

A single `Vista` reads one table from one driver and can traverse its own same-persistence
references (the typed table's `with_one` / `with_many`). It knows nothing about other datasources.
`VistaCatalog` sits one layer up: it holds every model in the system by name, builds any of them
regardless of driver, and follows relations whose target lives somewhere else (SQLite clients
pointing at bakeries in MongoDB, a REST resource pointing at a CSV file).

## Main types

- `ModelLoader`: `Arc<dyn Fn() -> Result<Vista> + Send + Sync>`. Called each time a model is built,
  so every Vista it returns starts without conditions.
- `VistaCatalog`: `register(name, loader)`, `register_relation(source_model, relation)`,
  `has_model`, `model_names`, `relations_for`, `build_vista(name)`.
- `Relation`: a cross-persistence relation with a target model name, a `ReferenceKind` and the join
  columns. Build one with `Relation::single_key(...)` or `Relation::multi_key(...)`.
  `relation.narrow(&mut vista, parent_row)` adds the eq-conditions to a Vista you built yourself.

## Example

```rust
use std::sync::Arc;
use vantage_vista::ReferenceKind;
use vantage_vista_factory::{Relation, VistaCatalog};

let mut cat = VistaCatalog::new();

let db = sqlite_db.clone();
cat.register("client", Arc::new(move || {
    db.vista_factory().from_table(Client::sqlite_table(db.clone()))
}));
let db = mongo_db.clone();
cat.register("bakery", Arc::new(move || {
    db.vista_factory().from_table(Bakery::mongo_table(db.clone()))
}));

// client.bakery_id -> bakery.id
cat.register_relation(
    "client",
    Relation::single_key("bakery", "bakery", ReferenceKind::HasOne, "id", "bakery_id"),
);

let rel = &cat.relations_for("client")[0];
let bakery = cat.traverse(rel, &client_row)?;
```

`traverse_from(parent, source_model, relation, row)` picks the route for you: it uses the parent
Vista's own `get_ref` when the parent declares the reference and reports `can_traverse_to_record`,
and falls back to the catalog relation otherwise.

## Integration

Depends only on `vantage-core`, `vantage-types` and `vantage-vista`. It has no driver or config
dependencies: callers fill the catalog from a YAML inventory, from typed tables or from test
fixtures. `vantage-diorama` uses it to resolve models by name. It
replaces the old `with_foreign` references that `vantage-table` used to offer.
