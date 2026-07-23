//! `KubeTableShell` — owns a `Table<KubernetesCluster, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! Reads delegate to the typed table (which projects every object into a
//! flat CBOR record). Conditions translate `(field, CborValue)` into a
//! `KubeCondition::Eq` pushed onto the table; the table source applies them
//! client-side at fetch time.
//!
//! The Kubernetes list API has no server-side ordering, pagination, or
//! free-text search, so those capabilities are honoured **client-side**:
//! [`materialize`](KubeTableShell::materialize) lists the (cluster-sized)
//! collection once, applies the quicksearch filter and the sort, and the
//! window / page / cursor methods slice the result. Writes stay unsupported
//! in v1 — only the read capabilities are advertised.
//!
//! ## Rhai
//!
//! No backend-specific `register_rhai_extensions` override is provided.
//! The projector already flattens every useful field — `phase`, `ready`,
//! `nodeName`, `app`, `cpuMillicores`, … — to top-level columns, which the
//! conventional Rhai verbs (`table()`, `get_ref()`, `list()`, `columns()`)
//! reach directly. A `label(row, key)` helper would just duplicate
//! `row.app`, so bespoke K8s vocabulary buys nothing here.

use std::cmp::Ordering;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, ReferenceKind, SortDirection, TableShell,
    Vista, VistaCapabilities, VistaMetadata,
};

use crate::cluster::KubernetesCluster;
use crate::condition::KubeCondition;

pub struct KubeTableShell {
    pub(crate) table: Table<KubernetesCluster, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Client-side sort. Vista's `add_order` is replace-semantics, so this
    /// holds at most one entry.
    orders: Vec<(String, SortDirection)>,
    /// Client-side quicksearch substring (case-insensitive).
    search: Option<String>,
    /// Page size for `fetch_page` / `fetch_next`.
    page_size: Option<usize>,
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
            orders: Vec::new(),
            search: None,
            page_size: None,
        }
    }

    /// List the collection (conditions + projection applied by the table
    /// source), then apply the client-side search filter and sort. The
    /// natural map order is preserved when no sort is set.
    async fn materialize(&self) -> Result<Vec<(String, Record<CborValue>)>> {
        let mut rows: Vec<(String, Record<CborValue>)> =
            self.table.list_values().await?.into_iter().collect();

        if let Some(needle) = &self.search {
            let needle = needle.to_lowercase();
            rows.retain(|(id, record)| row_matches(id, record, &needle));
        }

        for (field, dir) in &self.orders {
            // Stable sort keeps the prior order for equal keys, so the last
            // `add_order` is the primary key. (Vista applies one at a time.)
            rows.sort_by(|(_, a), (_, b)| {
                let ord = cmp_values(a.get(field), b.get(field));
                match dir {
                    SortDirection::Ascending => ord,
                    SortDirection::Descending => ord.reverse(),
                }
            });
        }

        Ok(rows)
    }
}

/// Quicksearch predicate: the needle (already lowercased) appears in the id
/// or in any text-valued field of the record.
fn row_matches(id: &str, record: &Record<CborValue>, needle: &str) -> bool {
    if id.to_lowercase().contains(needle) {
        return true;
    }
    record.values().any(|v| match v {
        CborValue::Text(s) => s.to_lowercase().contains(needle),
        _ => false,
    })
}

/// Order two optional CBOR values. Missing fields sort last; types compare
/// within kind, with a stable fallback across kinds.
fn cmp_values(a: Option<&CborValue>, b: Option<&CborValue>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater, // missing sorts last
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) => match (a, b) {
            (CborValue::Integer(x), CborValue::Integer(y)) => i128::from(*x).cmp(&i128::from(*y)),
            (CborValue::Float(x), CborValue::Float(y)) => x.partial_cmp(y).unwrap_or(Ordering::Equal),
            (CborValue::Text(x), CborValue::Text(y)) => x.cmp(y),
            (CborValue::Bool(x), CborValue::Bool(y)) => x.cmp(y),
            // Mixed/other kinds: fall back to a textual rendering so the sort
            // is total and deterministic rather than panicking.
            _ => format!("{a:?}").cmp(&format!("{b:?}")),
        },
    }
}

#[async_trait]
impl TableShell for KubeTableShell {
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(&self, _vista: &Vista) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(self.materialize().await?.into_iter().collect())
    }

    async fn get_vista_value(&self, _vista: &Vista, id: &String) -> Result<Option<Record<CborValue>>> {
        let mut data = self.table.list_values().await?;
        Ok(data.shift_remove(id))
    }

    async fn get_vista_some_value(&self, _vista: &Vista) -> Result<Option<(String, Record<CborValue>)>> {
        Ok(self.materialize().await?.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        // Honour the search filter — count what a listing would show.
        Ok(self.materialize().await?.len() as i64)
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        let all = self.materialize().await?;
        Ok(all.into_iter().skip(offset).take(limit).collect())
    }

    async fn fetch_page(&self, _vista: &Vista, page: usize) -> Result<Vec<(String, Record<CborValue>)>> {
        if page == 0 {
            return Err(error!("page is 1-based; got 0"));
        }
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_page"))?;
        let all = self.materialize().await?;
        Ok(all.into_iter().skip((page - 1) * size).take(size).collect())
    }

    async fn fetch_next(
        &self,
        _vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        let size = self
            .page_size
            .ok_or_else(|| error!("set_page_size must be called before fetch_next"))?;
        // Cursor is the 1-based page number (same shape as the SQL/Mongo drivers).
        let page: usize = match token {
            None => 1,
            Some(CborValue::Integer(n)) => usize::try_from(i128::from(n))
                .map_err(|_| error!("fetch_next token out of range"))?,
            Some(_) => return Err(error!("invalid fetch_next token type for kubernetes driver")),
        };
        if page < 1 {
            return Err(error!("fetch_next token must be a 1-based page number"));
        }

        let all = self.materialize().await?;
        let records: Vec<(String, Record<CborValue>)> =
            all.into_iter().skip((page - 1) * size).take(size).collect();
        let next_token = (records.len() == size).then(|| CborValue::Integer(((page + 1) as i64).into()));
        Ok((records, next_token))
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.table
            .add_condition(KubeCondition::eq(field.to_string(), value.clone()));
        Ok(())
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        // Replace semantics: the latest search wins.
        self.search = Some(text.to_string());
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        self.search = None;
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        if !self.metadata.columns.contains_key(field) {
            return Err(error!("Unknown column for add_order", field = field));
        }
        // Replace semantics, per the Vista contract.
        self.orders.clear();
        self.orders.push((field.to_string(), dir));
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.orders.clear();
        Ok(())
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        if size == 0 {
            return Err(error!("page size must be > 0"));
        }
        self.page_size = Some(size);
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let target = self.table.get_ref_from_row::<EmptyEntity>(relation, row)?;
        let factory = crate::vista::factory::KubeVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        let target = self.table.get_ref_target::<EmptyEntity>(relation)?;
        let factory = crate::vista::factory::KubeVistaFactory::new(self.table.data_source().clone());
        factory.from_table(target)
    }

    fn get_ref_kinds(&self) -> Vec<(String, ReferenceKind)> {
        self.table.ref_kinds()
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "kubernetes"
    }
}
