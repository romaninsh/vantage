//! Vista bridge for SpacetimeDB.
//!
//! Build a `Vista` from a table or view name via [`SpacetimeDb::vista_factory`],
//! which reads the module schema and derives the columns, the id column and the
//! capabilities from it — SpacetimeDB is fully self-describing, so nothing has
//! to be declared twice.
//!
//! Two ways in, and they meet at the same builder:
//!
//! - `factory.from_relation("account").await` — fetches the schema if needed,
//!   then builds. The convenient one from Rust.
//! - `factory.load().await?` then `from_yaml` / `build_from_spec` — the
//!   `VistaFactory` path, which is **synchronous** and so the only one a
//!   `VistaCatalog`, the CLI or a UI inventory can call. `load` does the
//!   fetching up front precisely so construction needs no `await`.

pub mod factory;
pub mod source;
pub mod spec;

pub use factory::SpacetimeVistaFactory;
pub use source::SpacetimeTableShell;
pub use spec::{SpacetimeColumnExtras, SpacetimeTableExtras, SpacetimeVistaSpec};

use crate::client::SpacetimeDb;

impl SpacetimeDb {
    /// A Vista factory bound to this database.
    pub fn vista_factory(&self) -> SpacetimeVistaFactory {
        SpacetimeVistaFactory::new(self.clone())
    }
}
