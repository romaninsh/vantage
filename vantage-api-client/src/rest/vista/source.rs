//! `RestApiTableShell` — owns the typed `Table<RestApi, EmptyEntity>`
//! and exposes it through the `TableShell` boundary.
//!
//! `RestApi` already speaks `ciborium::Value` natively (the HTTP body
//! is converted at the fetch boundary), so the shell is a pass-through
//! with no per-record translation.
//!
//! YAML-declared references are carried separately (`yaml_refs`)
//! because `Table::with_many` / `with_one` require compile-time-known
//! build-target closures that YAML can't synthesise. At traversal
//! time the shell reads the join value out of the parent row supplied
//! by `Vista::get_ref(relation, row)` and pushes a plain eq-condition
//! on the resolver-built child Vista — no deferred fetch.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, JoinKey, Reference as VistaReference, TableShell, Vista,
    VistaCapabilities, VistaMetadata,
};

use super::factory::{ModelResolver, RestApiVistaFactory};
use crate::RestApi;

/// A single YAML-declared reference attached to a parent shell at
/// build time. Carries the foreign-key wiring; the URL form is
/// the child's own concern (its `api.endpoint`) — the parent only
/// declares the relationship.
#[derive(Clone)]
pub(crate) struct YamlReference {
    pub target: String,
    pub kind: YamlReferenceKind,
    pub foreign_key: String,
    /// Multi-key join. When non-empty it fully describes the join (each
    /// pair reads a parent-row column and constrains a child column),
    /// superseding `foreign_key`. Lets a child be narrowed by more than
    /// one parent field — e.g. a deployment by both product_id and
    /// version_id.
    pub keys: Vec<JoinKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YamlReferenceKind {
    HasMany,
    HasOne,
}

pub struct RestApiTableShell {
    pub(crate) table: Table<RestApi, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    pub(crate) yaml_refs: IndexMap<String, YamlReference>,
    pub(crate) resolver: Option<ModelResolver>,
}

impl RestApiTableShell {
    pub(crate) fn new(
        table: Table<RestApi, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            yaml_refs: IndexMap::new(),
            resolver: None,
        }
    }

    pub(crate) fn with_yaml_refs(mut self, refs: IndexMap<String, YamlReference>) -> Self {
        self.yaml_refs = refs;
        self
    }

    pub(crate) fn with_resolver(mut self, resolver: ModelResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }
}

#[async_trait]
impl TableShell for RestApiTableShell {
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.table.list_values().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let mut data = self.table.list_values().await?;
        Ok(data.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.table.list_values().await?;
        Ok(data.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        // RestApi is not a SelectableDataSource, so reach the count via the
        // TableSource method directly (previously routed through TableLike).
        self.table.data_source().get_table_count(&self.table).await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let condition = crate::eq_condition(field, value.clone());
        self.table.add_condition(condition);
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        // YAML-declared references first. With row-based traversal we no
        // longer need the deferred-fetch dance — the parent record is on
        // hand, so we just read the join value out of `row` and push a
        // plain eq-condition on the child.
        if let Some(yref) = self.yaml_refs.get(relation) {
            let resolver = self.resolver.as_ref().ok_or_else(|| {
                error!(
                    "YAML reference requires a model resolver — call \
                     `with_model_resolver` on the factory or register \
                     the target spec via `register_yaml`",
                    relation = relation
                )
            })?;

            let mut child = resolver(&yref.target)?;

            // Multi-key join: read each parent-row column and constrain the
            // matching child column. Every key becomes an eq-condition on
            // the child; how each one is applied (URL path placeholder,
            // query param, or in-memory row filter) is the child data
            // source's concern at fetch time.
            if !yref.keys.is_empty() {
                for key in &yref.keys {
                    let value = row.get(&key.from).cloned().ok_or_else(|| {
                        error!(
                            "YAML reference: parent row missing join field",
                            relation = relation,
                            source_column = key.from.as_str()
                        )
                    })?;
                    child.add_condition_eq(key.to.clone(), value)?;
                }
                return Ok(child);
            }

            // For `has_many` the parent's id flows onto the child's FK
            // column; for `has_one` the parent's FK column value becomes
            // the child's id. The field we read from `row` differs
            // accordingly.
            let (source_column, target_field) = match yref.kind {
                YamlReferenceKind::HasMany => {
                    let parent_id = self
                        .table
                        .id_field()
                        .map(|c| c.name().to_string())
                        .ok_or_else(|| {
                            error!(
                                "YAML has_many reference needs the parent to have an id field",
                                relation = relation
                            )
                        })?;
                    (parent_id, yref.foreign_key.clone())
                }
                YamlReferenceKind::HasOne => {
                    let child_id = child
                        .get_id_column()
                        .map(str::to_string)
                        .unwrap_or_else(|| "id".to_string());
                    (yref.foreign_key.clone(), child_id)
                }
            };

            let join_value = row.get(&source_column).cloned().ok_or_else(|| {
                error!(
                    "YAML reference: parent row missing join field",
                    relation = relation,
                    source_column = source_column.as_str()
                )
            })?;

            child.add_condition_eq(target_field, join_value)?;

            return Ok(child);
        }

        // Hand-coded `with_many` / `with_one` registrations on the typed
        // table: resolve the target table from `row` directly, then wrap
        // it back into a Vista via a fresh factory bound to the same
        // data source.
        let target = self.table.get_ref_from_row::<EmptyEntity>(relation, row)?;
        let factory = RestApiVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "rest-api"
    }
}
