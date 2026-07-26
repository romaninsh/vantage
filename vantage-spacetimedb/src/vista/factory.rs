//! Building a `Vista` from a module's own schema.

use vantage_core::{Result, error};
use vantage_vista::{NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata};

use crate::client::SpacetimeDb;
use crate::schema::{ModuleSchema, TableKind};
use crate::vista::source::SpacetimeTableShell;
use crate::vista::spec::{SpacetimeColumnExtras, SpacetimeTableExtras, SpacetimeVistaSpec};

pub struct SpacetimeVistaFactory {
    db: SpacetimeDb,
    /// Cached module schema, so building several tables costs one fetch.
    schema: Option<ModuleSchema>,
    /// Cached ownership answer. Writes need the database owner, not merely a
    /// token, and finding out costs a request — so ask once.
    can_write: Option<bool>,
}

impl SpacetimeVistaFactory {
    pub fn new(db: SpacetimeDb) -> Self {
        Self {
            db,
            schema: None,
            can_write: None,
        }
    }

    /// Read the module schema and the ownership answer once, and hold both.
    pub async fn load(mut self) -> Result<Self> {
        self.schema = Some(self.db.module_schema().await?);
        self.can_write = Some(self.db.can_write().await?);
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
    ///
    /// Fetches the schema on first use if [`Self::load`] was not called; after
    /// that it is [`Self::build`] with the network already done.
    pub async fn from_relation(&mut self, name: &str) -> Result<Vista> {
        if self.schema.is_none() {
            self.schema = Some(self.db.module_schema().await?);
        }
        if self.can_write.is_none() {
            // A failure here means we could not establish ownership, so assume
            // read-only: advertising writes we cannot perform is the worse error.
            self.can_write = Some(self.db.can_write().await.unwrap_or(false));
        }
        self.build(name)
    }

    /// Build a Vista over an already-loaded schema, without touching the network.
    ///
    /// This is the shape the rest of Vantage can actually call. `build_from_spec`
    /// is `fn(&self)` and a `ModelLoader` is `Fn() -> Result<Vista>` — both
    /// synchronous — so an `async fn(&mut self)` constructor, whatever else it
    /// is, cannot be registered in a catalog, reached from the CLI, or named in
    /// a UI inventory. Doing the two requests once in [`Self::load`] and leaving
    /// construction pure is what reconciles the two.
    pub fn build(&self, name: &str) -> Result<Vista> {
        let metadata = self.metadata_for(name)?;
        self.assemble(name, metadata)
    }

    /// The metadata the module declares for a relation, before any narrowing.
    fn metadata_for(&self, name: &str) -> Result<VistaMetadata> {
        self.loaded(name)?.metadata_for(name)
    }

    fn loaded(&self, name: &str) -> Result<&ModuleSchema> {
        self.schema.as_ref().ok_or_else(|| {
            error!(
                "the module schema has not been read yet — call `load()` (or use \
                 `from_relation`, which fetches it) before building",
                relation = name.to_string()
            )
        })
    }

    /// Wrap a relation and its metadata in a Vista.
    ///
    /// Split from [`Self::build`] so a spec can narrow the metadata on the way
    /// through: a `Vista` exposes no way to replace it afterwards.
    fn assemble(&self, name: &str, metadata: VistaMetadata) -> Result<Vista> {
        let schema = self.loaded(name)?;
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
                capabilities_for(table.kind, self.can_write.unwrap_or(false)),
            )),
        ))
    }
}

/// Building from YAML.
///
/// Deliberately thin. The module already declares columns, types, keys and
/// whether a relation is a table or a view, so a spec that restated them would
/// be a second source of truth that drifts the moment someone republishes. What
/// the spec contributes is the *name* — which relation, called what.
///
/// Anything it declares that this driver cannot lower is refused rather than
/// ignored. A silently dropped `computed:` block is a column that simply is not
/// there, with nothing to say why.
impl VistaFactory for SpacetimeVistaFactory {
    type TableExtras = SpacetimeTableExtras;
    type ColumnExtras = SpacetimeColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: SpacetimeVistaSpec) -> Result<Vista> {
        let relation = spec.driver.relation.as_deref().unwrap_or(&spec.name);

        if !spec.references.is_empty() {
            return Err(error!(
                "a SpacetimeDB module declares no foreign keys, so references cannot be \
                 resolved against it",
                relation = relation.to_string()
            )
            .mark_unsupported());
        }
        if !spec.contained.is_empty() {
            return Err(error!(
                "contained relations are not lowered by this driver — a nested SATS product \
                 reaches a Vista as one `json` column",
                relation = relation.to_string()
            )
            .mark_unsupported());
        }
        for (name, column) in &spec.columns {
            if column.lazy.is_some() || column.expr.is_some() {
                return Err(error!(
                    "computed and lazy columns need an expression vocabulary this driver does \
                     not have",
                    relation = relation.to_string(),
                    column = name.to_string()
                )
                .mark_unsupported());
            }
            if column.references.is_some() {
                return Err(error!(
                    "a column cannot declare a reference: the module has no foreign keys",
                    relation = relation.to_string(),
                    column = name.to_string()
                )
                .mark_unsupported());
            }
        }

        let declared_by_module = self.metadata_for(relation)?;

        // The module's key is authoritative. An override that disagrees would
        // address rows by a column the database does not key that way, so say so
        // rather than quietly honouring one or the other.
        if let Some(declared) = &spec.id_column
            && declared_by_module.id_column.as_deref() != Some(declared.as_str())
        {
            return Err(error!(
                "the spec's id_column is not the relation's key",
                relation = relation.to_string(),
                declared = declared.clone(),
                actual = declared_by_module
                    .id_column
                    .clone()
                    .unwrap_or_else(|| "none".into())
            ));
        }

        // A spec that lists columns is narrowing the view, which is the one
        // genuinely useful thing it can add. Every name has to exist in the
        // module — a typo that silently showed nothing would read as an empty
        // table, and the author would go looking at the database.
        let metadata = if spec.columns.is_empty() {
            declared_by_module
        } else {
            let mut narrowed = VistaMetadata::new();
            narrowed.id_column = declared_by_module.id_column.clone();
            for name in spec.columns.keys() {
                let column = declared_by_module.columns.get(name).ok_or_else(|| {
                    error!(
                        "the spec names a column the module does not declare",
                        relation = relation.to_string(),
                        column = name.clone()
                    )
                })?;
                narrowed = narrowed.with_column(column.clone());
            }
            narrowed
        };

        let mut vista = self.assemble(relation, metadata)?;
        vista.set_name(spec.name);
        Ok(vista)
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
/// - `can_traverse_to_record` — the *dialect* could do it, since it is only an
///   eq-condition on a known value, but a SpacetimeDB module declares no foreign
///   keys: the ABI has primary keys, unique constraints and indexes, and nothing
///   that says one column points at another table. So [`ModuleSchema`] builds no
///   references, `VistaMetadata::references` is always empty, and there is
///   nothing to traverse. Advertising it would promise a road with no map.
///
/// `can_filter_operators` is `true` because `WHERE` supports the six comparison
/// operators; `InSet`/`NotInSet` are refused individually, since the flag covers
/// operators as a class rather than each one.
///
/// The write flags key off **database ownership**, not merely holding a token:
/// SpacetimeDB refuses SQL DML from any caller who is not the owner, so deciding
/// from `has_token()` would advertise writes that always fail.
fn capabilities_for(kind: TableKind, caller_is_owner: bool) -> VistaCapabilities {
    // A view has nothing to write to, regardless of credentials.
    let writable = caller_is_owner && matches!(kind, TableKind::Table);
    VistaCapabilities {
        can_count: true,
        can_filter_operators: true,
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
    fn tables_are_writable_only_for_the_database_owner() {
        // Not "has a token": SpacetimeDB refuses SQL DML from any caller who is
        // not the owner, so a token alone must not turn these on.
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
        // A SpacetimeDB module declares no foreign keys, so `VistaMetadata`
        // carries no references and there is nothing to traverse to. This was
        // `true` while `get_ref` was never implemented — the framework treats
        // that combination as a driver bug and traces it at error level, which
        // is precisely the "capability that lies" this crate argues against.
        assert!(
            !caps.can_traverse_to_record,
            "no foreign keys in the module definition, so no references to follow"
        );
    }

    /// Every `true` flag must have an implementation behind it.
    ///
    /// Kept as a list rather than a prose comment because the failure mode is
    /// silent: a flag is one word, the method it promises is somewhere else
    /// entirely, and nothing but a reader connects them. Both flags that had
    /// drifted apart from their methods were `true` ones.
    #[test]
    fn every_advertised_capability_has_a_method_behind_it() {
        let caps = capabilities_for(TableKind::Table, true);

        // can_count            -> get_vista_count           (source.rs)
        // can_filter_operators -> add_op_condition          (source.rs)
        // can_subscribe        -> watch_vista               (source.rs)
        // can_insert           -> insert_vista_value        (source.rs)
        // can_update           -> patch/replace_vista_value (source.rs)
        // can_delete           -> delete_vista_value        (source.rs)
        assert!(caps.can_count);
        assert!(caps.can_filter_operators);
        assert!(caps.can_subscribe);
        assert!(caps.can_insert && caps.can_update && caps.can_delete);

        // The one deliberate gap: `can_insert` covers `insert_vista_value`,
        // where the caller supplies the id. `insert_vista_return_id_value` — the
        // server-assigns-it path — is overridden to refuse, because SpacetimeDB
        // has no `RETURNING` to read the assigned id back from. Overridden
        // rather than left to the default, so it reports Unsupported instead of
        // "the driver forgot".
    }
}
