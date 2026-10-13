//! `KubeTableShell` — owns a `Table<KubernetesCluster, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! Reads delegate to the typed table, which projects every object into a
//! flat CBOR record. Equality conditions become `KubeCondition::Eq` on the
//! table; the table source applies them client-side at fetch time, and a
//! record missing the field doesn't match.
//!
//! The Kubernetes list API has no server-side ordering, pagination or
//! free-text search, so the shell honours them **client-side**: every read
//! lists the (cluster-sized) collection once, applies the quicksearch filter
//! and the sort (see [`refine`]), and windows slice the result. The search
//! text and order live in plain fields, so a clone carries them. Writes stay
//! unsupported; only read capabilities are advertised.
//!
//! ## Rhai
//!
//! No backend-specific `register_rhai_extensions` override is provided.
//! The projector already flattens every useful field — `phase`, `ready`,
//! `nodeName`, `app`, `cpuMillicores`, … — to top-level columns, which the
//! conventional Rhai verbs reach directly.

mod refine;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Condition, FilterOp, Page, Preview, SortDirection, TableShell, Vista,
    VistaCapabilities, VistaMetadata, unknown_relation, unsupported,
};

use crate::cluster::KubernetesCluster;
use crate::condition::KubeCondition;
use crate::vista::factory::KubeVistaFactory;

#[derive(Clone)]
pub struct KubeTableShell {
    pub(crate) table: Table<KubernetesCluster, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Client-side sort over the listed rows.
    order: Option<(String, SortDirection)>,
    /// Client-side quicksearch substring (case-insensitive).
    search: Option<String>,
}

impl KubeTableShell {
    pub(crate) fn new(
        table: Table<KubernetesCluster, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
            order: None,
            search: None,
        }
    }

    /// The set in its order: the collection listed with its conditions, then
    /// searched and sorted client-side.
    async fn materialize(&self) -> Result<Vec<(String, Record<CborValue>)>> {
        let rows = self.table.list_values().await?.into_iter().collect();
        Ok(refine::refine(
            rows,
            self.search.as_deref(),
            self.order.as_ref(),
        ))
    }
}

#[async_trait]
impl TableShell for KubeTableShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "kubernetes"
    }

    /// The collection this shell lists, plus the narrowing it applies after
    /// the API answers. Conditions, search and ordering never reach the API
    /// server.
    fn preview(&self) -> Preview {
        Preview::new()
            .with_query(format!("GET {}", self.table.table_name()))
            .with_note(refine::describe(
                self.table.conditions().map(KubeCondition::field),
                self.search.as_deref(),
                self.order.as_ref(),
            ))
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(self.materialize().await?.into_iter().collect())
    }

    /// Row `id` of the refined listing, so the search applies.
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(refine::pick(self.materialize().await?, id))
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.materialize().await?.len() as i64)
    }

    /// The window is cut from the refined listing, which is read whole
    /// anyway, so the page carries the set's total for free.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(self.materialize().await?, offset, limit))
    }

    /// Equality on a declared column, matched against the projected record.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if !self.metadata.columns.contains_key(field) {
            return Err(error!("Unknown column for condition", field = field));
        }
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        self.table
            .add_condition(KubeCondition::eq(field, condition.value.clone()));
        Ok(())
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.search = text.map(str::to_string);
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        if let Some((field, _)) = order
            && !self.metadata.columns.contains_key(field)
        {
            return Err(error!("Unknown column for order", field = field));
        }
        self.order = order.map(|(field, dir)| (field.to_string(), dir));
        Ok(())
    }

    /// The target comes from the table's `with_one`/`with_many`; from a row
    /// it is narrowed by the reference's join keys.
    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| unknown_relation(relation))?;
        let target = self.table.get_ref_target_erased(relation)?;
        let target = KubeVistaFactory::new(self.table.data_source().clone()).from_table(target)?;
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
    }
}
