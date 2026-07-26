//! Building a `Vista` from a module's own schema.

use vantage_core::{Result, error};
use vantage_vista::{Vista, VistaCapabilities};

use crate::client::SpacetimeDb;
use crate::schema::{ModuleSchema, TableKind};
use crate::vista::source::SpacetimeTableShell;

pub struct SpacetimeVistaFactory {
    db: SpacetimeDb,
    /// Cached module schema, so building several tables costs one fetch.
    schema: Option<ModuleSchema>,
}

impl SpacetimeVistaFactory {
    pub fn new(db: SpacetimeDb) -> Self {
        Self { db, schema: None }
    }

    /// Read the module schema once and hold it for subsequent builds.
    pub async fn load(mut self) -> Result<Self> {
        self.schema = Some(self.db.module_schema().await?);
        Ok(self)
    }

    pub fn schema(&self) -> Option<&ModuleSchema> {
        self.schema.as_ref()
    }

    /// Build a Vista over one table or view.
    ///
    /// Every property comes from the module definition: columns and types, the id
    /// column, and whether the relation is writable. Nothing needs declaring in
    /// YAML unless an author wants to narrow or re-label it.
    pub async fn from_relation(&mut self, name: &str) -> Result<Vista> {
        if self.schema.is_none() {
            self.schema = Some(self.db.module_schema().await?);
        }
        let schema = self.schema.as_ref().expect("just loaded");

        let metadata = schema.metadata_for(name)?;
        let table = schema
            .tables
            .get(name)
            .ok_or_else(|| error!("unknown relation", relation = name.to_string()))?;

        Ok(Vista::new(
            name.to_string(),
            Box::new(SpacetimeTableShell::new(
                self.db.clone(),
                name.to_string(),
                metadata,
                table.row_identity(),
                table.row_product_type(),
                capabilities_for(table.kind, self.db.has_token()),
            )),
        ))
    }
}

/// The honest capability set for a SpacetimeDB relation.
///
/// Everything `false` here is false because the server genuinely cannot do it,
/// and this driver does not emulate:
///
/// - `can_order` — the dialect has no `ORDER BY`.
/// - `can_search` — no `LIKE`, so no substring match to push down.
/// - `can_fetch_page` / `can_fetch_next` / `can_fetch_window` — `LIMIT` exists
///   but `OFFSET` does not, so there is no way to skip to a window. With all
///   three `false`, consumers fall through to `list_values`, which is the
///   documented meaning of "no native pagination".
/// - `can_traverse_to_set` / `can_traverse_in_columns` — both need subqueries.
///
/// `can_filter_operators` is `true` because `WHERE` supports the six comparison
/// operators; `InSet`/`NotInSet` are refused individually, since the flag covers
/// operators as a class rather than each one.
fn capabilities_for(kind: TableKind, has_token: bool) -> VistaCapabilities {
    // A view has nothing to write to, regardless of credentials.
    let writable = has_token && matches!(kind, TableKind::Table);
    VistaCapabilities {
        can_count: true,
        can_filter_operators: true,
        can_traverse_to_record: true,
        // Subscriptions need no setup on this backend — no trigger to install, no
        // extension to enable — so advertising push is honest without an opt-in.
        // Contrast `PostgresVistaFactory::with_notify`.
        can_subscribe: true,
        can_insert: writable,
        can_update: writable,
        can_delete: writable,
        ..VistaCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_are_never_writable_even_with_a_token() {
        let caps = capabilities_for(TableKind::View, true);
        assert!(!caps.can_insert && !caps.can_update && !caps.can_delete);
        assert!(caps.can_subscribe, "views are still subscribable");
    }

    #[test]
    fn tables_are_writable_only_with_a_token() {
        assert!(capabilities_for(TableKind::Table, true).can_insert);
        assert!(!capabilities_for(TableKind::Table, false).can_insert);
    }

    #[test]
    fn the_capabilities_this_backend_lacks_stay_false() {
        // Pinning these prevents someone "helpfully" emulating them later: the
        // flags and the behaviour have to move together.
        let caps = capabilities_for(TableKind::Table, true);
        assert!(!caps.can_order, "no ORDER BY in the dialect");
        assert!(!caps.can_search, "no LIKE in the dialect");
        assert!(!caps.can_fetch_page && !caps.can_fetch_next && !caps.can_fetch_window);
        assert!(!caps.can_traverse_to_set && !caps.can_traverse_in_columns);
    }
}
