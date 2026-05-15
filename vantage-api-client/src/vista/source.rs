//! `RestApiTableShell` — owns the typed `Table<RestApi, E>` behind a
//! `dyn TableLike` so the original entity type stays attached for
//! reference traversal.
//!
//! `RestApi` already speaks `ciborium::Value` natively (the HTTP body
//! is converted at the fetch boundary), so the shell is a pass-through
//! with no per-record translation.
//!
//! YAML-declared references are carried separately (`yaml_refs`)
//! because `Table::with_many` / `with_one` require compile-time-known
//! build-target closures that YAML can't synthesise. At traversal
//! time the shell consults the resolver (a factory-wide callback that
//! maps model names to child Vistas) and threads a `DeferredFn`
//! through the child so the parent's id resolves only when the child
//! actually fetches.

use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};
use vantage_table::traits::table_like::TableLike;
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use crate::vista::factory::ModelResolver;

use super::any_shell::AnyTableShell;

/// A single YAML-declared reference attached to a parent shell at
/// build time. Carries the foreign-key wiring; the URL form is
/// the child's own concern (its `api.endpoint`) — the parent only
/// declares the relationship.
#[derive(Clone)]
pub(crate) struct YamlReference {
    pub target: String,
    pub kind: YamlReferenceKind,
    pub foreign_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YamlReferenceKind {
    HasMany,
    HasOne,
}

pub struct RestApiTableShell {
    pub(crate) table: Box<dyn TableLike<Value = CborValue, Id = String>>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) yaml_refs: IndexMap<String, YamlReference>,
    pub(crate) resolver: Option<ModelResolver>,
}

impl RestApiTableShell {
    pub(crate) fn new(
        table: Box<dyn TableLike<Value = CborValue, Id = String>>,
        capabilities: VistaCapabilities,
    ) -> Self {
        Self {
            table,
            capabilities,
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

    /// Construct the deferred FK condition for a YAML reference. The
    /// returned `Expression` carries a `DeferredFn` that, when
    /// resolved at child-fetch time, reads `source_column` out of the
    /// parent's first record and emits it as a scalar — same shape
    /// `condition_to_query_param` already peels for synchronous eq
    /// conditions.
    fn build_deferred_fk_condition(
        parent_table: Box<dyn TableLike<Value = CborValue, Id = String>>,
        source_column: String,
        target_field: String,
    ) -> Expression<CborValue> {
        let parent_arc = Arc::new(parent_table);
        let column = source_column;
        let target_field_for_error = target_field.clone();
        let deferred = DeferredFn::new(move || {
            let parent = parent_arc.clone_box();
            let column = column.clone();
            let target = target_field_for_error.clone();
            Box::pin(async move {
                let records = parent.list_values().await?;
                let value = records
                    .values()
                    .next()
                    .and_then(|r| r.get(&column))
                    .cloned()
                    .ok_or_else(|| {
                        error!(
                            "YAML reference: parent yielded no row or column missing",
                            source_column = column,
                            target_field = target
                        )
                    })?;
                Ok(ExpressiveEnum::Scalar(value))
            })
        });

        Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(Expression::new(target_field, vec![])),
                ExpressiveEnum::Nested(Expression::new(
                    "{}",
                    vec![ExpressiveEnum::Deferred(deferred)],
                )),
            ],
        )
    }
}

#[async_trait]
impl TableShell for RestApiTableShell {
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
        self.table.get_count().await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // Build a typed `Expression<CborValue>` and hand it through
        // the type-erased `add_condition` API — `Table::add_condition`
        // downcasts it back to `RestApi::Condition` on the other side.
        let condition = crate::eq_condition(field, value.clone());
        self.table.add_condition(Box::new(condition))
    }

    fn add_raw_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        self.table.add_condition(condition)
    }

    fn get_ref(&self, relation: &str) -> Result<Vista> {
        // YAML-declared references first: the factory wired them via
        // `yaml_refs` at build time. Resolve them through the
        // model-resolver callback so cross-driver lookups (vantage-ui's
        // inventory) work alongside same-driver ones.
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

            // For `has_many` the parent's id flows onto the child's FK
            // column; for `has_one` the parent's FK column value
            // becomes the child's id. The source column we read from
            // the parent at fetch time differs accordingly.
            let (source_column, target_field) = match yref.kind {
                YamlReferenceKind::HasMany => {
                    let parent_id = self.table.id_field_name().ok_or_else(|| {
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

            let parent_clone = self.table.clone_box();
            let condition =
                Self::build_deferred_fk_condition(parent_clone, source_column, target_field);
            child.add_raw_condition(condition)?;

            return Ok(child);
        }

        // Fall through to the typed `Table` reference machinery —
        // hand-coded `with_many` / `with_one` / `with_foreign`
        // registrations in Rust callers go through this path.
        let any_table = self.table.get_ref(relation)?;
        AnyTableShell::into_vista(any_table)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "rest-api"
    }
}
