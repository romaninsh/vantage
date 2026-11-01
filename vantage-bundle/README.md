# vantage-bundle

The trait a compiled-in model crate implements so an application can serve
its tables and actions. A Vantage application normally reads its tables from
YAML. A bundle is the other source: a Rust crate compiled into the binary
that hands over typed tables and the operations that go with them. The
application resolves a bundle table the same way as a declared one.

The trait lives in its own crate so the model crate depends on it, not on the
application.

```toml
[dependencies]
vantage-bundle = "0.7.0"
```

```rust,ignore
use bakery_model3::{Client, SurrealDB};
use vantage_bundle::{BundleTable, ModelBundle};
use vantage_vista::Vista;

struct Bakery;

impl ModelBundle for Bakery {
    type Connection = SurrealDB;

    fn name(&self) -> &str {
        "bakery"
    }

    fn tables(&self) -> Vec<BundleTable> {
        vec![BundleTable {
            key: "client".into(),
            datasource: "bakery".into(),
            title: Some("Clients".into()),
        }]
    }

    fn build_vista(&self, table_key: &str, db: &SurrealDB) -> vantage_core::Result<Vista> {
        match table_key {
            "client" => db.vista_factory().from_table(Client::surreal_table(db.clone())),
            other => Err(vantage_core::error!("unknown table", table = other)),
        }
    }
}

// The application holds bundles as trait objects with the connection pinned:
let registry: Vec<Arc<dyn ModelBundle<Connection = SurrealDB>>> = vec![Arc::new(Bakery)];
```

## Main types

- `ModelBundle`: `name()`, `tables()`, `build_vista(table_key, &Connection)`,
  and two optional methods, `model_actions(&connections)` and `actions()`.
- `type Connection` is an associated type, not a fixed database handle. A
  SurrealDB bundle sets it to `SurrealDB`; the application keeps the
  connections and passes the right one in.
- `BundleTable { key, datasource, title }`. It has no schema on purpose: the
  application reads columns, flags and references off the Vista that
  `build_vista` returns, so the model crate stays the only source.
- `model_actions` returns `Arc<dyn vantage_action::ModelAction>`, typed
  operations with their own input and output schemas. It is called on every
  (re)connect, keyed by datasource.
- `BuiltinAction` is the older untyped form (JSON in, JSON out, fields
  declared by the screen). Prefer `model_actions` for new code.

## Related crates

`vantage-vista` provides `Vista`. `vantage-action` provides `ModelAction`.
