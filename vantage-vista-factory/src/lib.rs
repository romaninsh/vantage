//! `vantage-vista-factory` — a multi-datasource [`Vista`] catalog and the home of
//! **cross-persistence** reference traversal.
//!
//! A single [`Vista`] reads one table from one driver and can traverse its
//! own *same-persistence* references (forwarded to the typed `Table`'s
//! `with_one`/`with_many`, gated by
//! [`VistaCapabilities::can_traverse_to_record`](vantage_vista::VistaCapabilities)).
//! It deliberately knows nothing about *other* datasources.
//!
//! [`VistaCatalog`] sits one layer up: it is given the set of models in the
//! system (a "folder of models", resolved to name → loader), can build any of
//! them by name regardless of which driver backs it, and traverses references
//! whose target lives in a *different* persistence. It is the single home for
//! what used to be three or four separate name→Vista resolvers:
//! `vantage-cli-util`'s `ModelFactory`, `vantage-api-client`'s `ModelResolver`,
//! and the UI backend's `ResolverContext::load_target_vista`.
//!
//! The catalog is deliberately **config-agnostic** and **driver-agnostic**:
//! it holds [`ModelLoader`] closures keyed by model name (registration-based,
//! so no dependency on any driver crate or on a particular config schema).
//! Callers populate it however they like — from a YAML inventory folder, from
//! hand-built typed tables, or from a test fixture.

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{ReferenceKind, Vista};

/// Produces a fresh, unconditioned target [`Vista`] on demand.
///
/// One per model name. Called every time a model is built or traversed to, so
/// the returned Vista is always pristine — the caller (or [`VistaCatalog`]) is
/// free to narrow it with conditions afterwards. This is the same role filled
/// today by `ModelFactory::for_name` (CLI), `ModelResolver` (api-client), and
/// `ResolverContext::load_target_vista` (UI).
pub type ModelLoader = Arc<dyn Fn() -> Result<Vista> + Send + Sync>;

/// A cross-persistence relation: the target is another model resolved **by
/// name** through the catalog, then narrowed by reading join values out of a
/// known parent row.
///
/// Mirrors the UI backend's `UiRelation` so lowering a YAML `references:` /
/// `has_many:` block into this type is a field-for-field copy.
#[derive(Clone, Debug)]
pub struct Relation {
    /// Relation name as the consumer addresses it (`"bakery"`, `"orders"`).
    pub name: String,
    /// Catalog key of the target model this relation resolves to.
    pub target_model: String,
    /// Cardinality — drives whether a consumer renders a record or a list.
    pub kind: ReferenceKind,
    /// `(child_column, parent_column)` pairs for a multi-key join. Empty for
    /// single-key relations, which use [`foreign_key`](Self::foreign_key) /
    /// [`narrow_via`](Self::narrow_via) instead.
    pub keys: Vec<(String, String)>,
    /// Single-key: the target column constrained by the parent's value.
    pub foreign_key: String,
    /// Single-key: the parent-row field whose value is read for the join.
    pub narrow_via: String,
}

impl Relation {
    /// A single-key relation: `target.foreign_key == parent_row[narrow_via]`.
    pub fn single_key(
        name: impl Into<String>,
        target_model: impl Into<String>,
        kind: ReferenceKind,
        foreign_key: impl Into<String>,
        narrow_via: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            target_model: target_model.into(),
            kind,
            keys: Vec::new(),
            foreign_key: foreign_key.into(),
            narrow_via: narrow_via.into(),
        }
    }

    /// A multi-key relation: every `(child, parent)` pair must match.
    pub fn multi_key(
        name: impl Into<String>,
        target_model: impl Into<String>,
        kind: ReferenceKind,
        keys: Vec<(String, String)>,
    ) -> Self {
        Self {
            name: name.into(),
            target_model: target_model.into(),
            kind,
            keys,
            foreign_key: String::new(),
            narrow_via: String::new(),
        }
    }
}

/// A name → [`Vista`] catalog spanning many datasources, plus
/// cross-persistence reference traversal between the models it holds.
#[derive(Default, Clone)]
pub struct VistaCatalog {
    loaders: IndexMap<String, ModelLoader>,
    /// Cross-persistence relations keyed by *source* model name.
    relations: HashMap<String, Vec<Relation>>,
}

impl VistaCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register how to build a model by name. Replaces any prior loader for
    /// the same name.
    pub fn register(&mut self, model: impl Into<String>, loader: ModelLoader) -> &mut Self {
        self.loaders.insert(model.into(), loader);
        self
    }

    /// Register a cross-persistence relation that originates from `source_model`.
    pub fn register_relation(
        &mut self,
        source_model: impl Into<String>,
        relation: Relation,
    ) -> &mut Self {
        self.relations
            .entry(source_model.into())
            .or_default()
            .push(relation);
        self
    }

    /// Whether a model with this name is registered.
    pub fn has_model(&self, name: &str) -> bool {
        self.loaders.contains_key(name)
    }

    /// All registered model names, in registration order.
    pub fn model_names(&self) -> impl Iterator<Item = &str> {
        self.loaders.keys().map(String::as_str)
    }

    /// Cross-persistence relations registered for a source model.
    pub fn relations_for(&self, source_model: &str) -> &[Relation] {
        self.relations
            .get(source_model)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Build a fresh, unconditioned [`Vista`] for a model.
    pub fn build_vista(&self, name: &str) -> Result<Vista> {
        let loader = self
            .loaders
            .get(name)
            .ok_or_else(|| error!("model not in catalog", model = name))?;
        loader()
    }

    /// Resolve a cross-persistence [`Relation`] from a known parent row into a
    /// narrowed target [`Vista`].
    ///
    /// Builds the target by name, then constrains it by every join key, reading
    /// each parent value out of `parent_row`. How each eq-condition is honoured
    /// (SQL `WHERE`, in-memory filter, REST path/query param) is the target
    /// driver's concern at fetch time.
    pub fn traverse(&self, relation: &Relation, parent_row: &Record<CborValue>) -> Result<Vista> {
        let mut target = self.build_vista(&relation.target_model)?;
        if relation.keys.is_empty() {
            let value = parent_row.get(&relation.narrow_via).cloned().ok_or_else(|| {
                error!(
                    "parent row missing narrow_via field",
                    field = relation.narrow_via.as_str()
                )
            })?;
            target.add_condition_eq(relation.foreign_key.clone(), value)?;
        } else {
            for (child_col, parent_col) in &relation.keys {
                let value = parent_row.get(parent_col).cloned().ok_or_else(|| {
                    error!("parent row missing join field", field = parent_col.as_str())
                })?;
                target.add_condition_eq(child_col.clone(), value)?;
            }
        }
        Ok(target)
    }

    /// Unified traversal from a parent [`Vista`] + row.
    ///
    /// Prefers the parent's own **same-persistence** reference when the parent
    /// shell declares it and advertises
    /// [`can_traverse_to_record`](vantage_vista::VistaCapabilities::can_traverse_to_record)
    /// — that path stays entirely inside one driver. Otherwise falls back to a
    /// registered **cross-persistence** [`Relation`] for `source_model`.
    pub fn traverse_from(
        &self,
        parent: &Vista,
        source_model: &str,
        relation: &str,
        parent_row: &Record<CborValue>,
    ) -> Result<Vista> {
        let same_persistence = parent.capabilities().can_traverse_to_record
            && parent.get_references().iter().any(|r| r == relation);
        if same_persistence {
            return parent.get_ref(relation, parent_row);
        }
        let rel = self
            .relations_for(source_model)
            .iter()
            .find(|r| r.name == relation)
            .ok_or_else(|| {
                error!(
                    "relation not found in catalog",
                    relation = relation,
                    model = source_model
                )
            })?;
        self.traverse(rel, parent_row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_dataset::ReadableValueSet;
    use vantage_vista::mocks::MockShell;
    use vantage_vista::{Column, VistaMetadata};

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.into())
    }

    fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
        let mut r = Record::new();
        for (k, v) in pairs {
            r.insert((*k).to_string(), v.clone());
        }
        r
    }

    /// Two models in two (notionally different) stores: `client` and `bakery`.
    /// A client carries `bakery_id`; bakery is keyed by `id`.
    fn catalog() -> VistaCatalog {
        let mut cat = VistaCatalog::new();

        cat.register(
            "bakery",
            Arc::new(|| {
                let meta = VistaMetadata::new()
                    .with_column(Column::new("id", "String").with_flag("id"))
                    .with_column(Column::new("name", "String"))
                    .with_id_column("id");
                let source = MockShell::new()
                    .with_metadata(meta)
                    .with_record("b1", record(&[("id", text("b1")), ("name", text("Marty's"))]))
                    .with_record("b2", record(&[("id", text("b2")), ("name", text("Other"))]));
                Ok(Vista::new("bakery", Box::new(source)))
            }),
        );

        cat.register(
            "client",
            Arc::new(|| {
                let meta = VistaMetadata::new()
                    .with_column(Column::new("id", "String").with_flag("id"))
                    .with_column(Column::new("bakery_id", "String"))
                    .with_id_column("id");
                let source = MockShell::new().with_metadata(meta);
                Ok(Vista::new("client", Box::new(source)))
            }),
        );

        cat.register_relation(
            "client",
            Relation::single_key("bakery", "bakery", ReferenceKind::HasOne, "id", "bakery_id"),
        );
        cat
    }

    #[test]
    fn build_vista_resolves_by_name() {
        let cat = catalog();
        assert!(cat.has_model("bakery"));
        assert!(!cat.has_model("nope"));
        let v = cat.build_vista("bakery").unwrap();
        assert_eq!(v.name(), "bakery");
        assert!(cat.build_vista("nope").is_err());
    }

    #[tokio::test]
    async fn traverse_cross_persistence_narrows_target() {
        let cat = catalog();
        let marty = record(&[("id", text("c1")), ("bakery_id", text("b1"))]);
        let rel = &cat.relations_for("client")[0];

        let bakery = cat.traverse(rel, &marty).unwrap();
        // The eq-condition on bakery.id = "b1" must select exactly one row.
        let rows = bakery.list_values().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows.contains_key("b1"));
    }

    #[test]
    fn traverse_missing_join_field_errors() {
        let cat = catalog();
        let rel = &cat.relations_for("client")[0];
        let bad = record(&[("id", text("c1"))]); // no bakery_id
        assert!(cat.traverse(rel, &bad).is_err());
    }
}
