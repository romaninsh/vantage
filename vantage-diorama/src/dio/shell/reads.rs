use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{
    Condition, Page, Preview, SortDirection, TableShell, VistaCapabilities, VistaChangeStream,
    VistaMetadata, VistaQuery,
};

use super::ReadPlan;
use crate::dio::admitted_by_dio;
use crate::dio::shell::DioShell;

type Rows = IndexMap<String, Record<CborValue>>;

#[async_trait]
impl TableShell for DioShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    // ---- Reads: cache-first; bounded reads hydrate ----------------------------
    //
    // Every read keeps only the rows the Dio's residual admits, and evaluates
    // whatever of the handle's query the master didn't take.
    //
    // When the Dio carries augmentations, a bounded facade read (`get`,
    // `fetch_window`) runs the detail pass for every returned row that still
    // has an augment gap: the read blocks until its rows are hydrated (and
    // cached, so the cost is paid once). `list` stays cheap: a listing is the
    // fast spine, and hydrating an entire set through it means one
    // innocent-looking call downloading everything. Ask for a window when you
    // want details.

    async fn list(&self) -> Result<Rows> {
        self.read().await
    }

    /// A narrowed handle is a smaller set: an id outside it is not found,
    /// rather than found and silently outside the filter. A cached row is
    /// hydrated before it is judged, so a clause on an augmented column sees
    /// the augmented value.
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        let ReadPlan { source, local } = self.plan()?;
        let Some(master) = source else {
            return self.cached_in_view(id, true).await;
        };
        let Some(row) = master.list_values().await?.shift_remove(id) else {
            return Ok(None);
        };
        let rows = IndexMap::from([(id.to_string(), row)]);
        Ok(self.refine(rows, &local, true)?.shift_remove(id))
    }

    async fn count(&self) -> Result<i64> {
        if self.reads_whole_cache() {
            return self.dio.cache.count().await;
        }
        Ok(self.read().await?.len() as i64)
    }

    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let all = self.read().await?;
        let total = all.len() as i64;
        let mut rows: Rows = all.into_iter().skip(offset).take(limit).collect();
        self.hydrate(&mut rows).await?;
        Ok(Page::new(rows.into_iter().collect()).with_total(total))
    }

    // ---- Narrowing ------------------------------------------------------------
    //
    // Recorded into the handle's query and routed at read time (see
    // `DioShell::plan`). Conditions accumulate; search and order replace. A
    // clone carries the narrowing but owns it, so narrowing a derived handle
    // never disturbs the one it came from.

    /// Takes any operator on a facade column, an augmented column, or a
    /// dotted path into a facade column; any other column is an error. A
    /// facade over a master with no declared columns can't check, and takes
    /// any.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let column = condition.column.as_str();
        let root = column.split('.').next().unwrap_or(column);
        let columns = &self.metadata.columns;
        if !columns.is_empty()
            && !columns.contains_key(column)
            && !columns.contains_key(root)
            && !self.dio.is_augmented_column(column)
        {
            return Err(error!(
                "Unknown column",
                column = column,
                table = self.dio.cache_table_name
            ));
        }
        self.query.conditions.push(condition.clone());
        Ok(())
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.query.search = text.map(str::to_string);
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.query.order = order.map(|(field, dir)| (field.to_string(), dir));
        Ok(())
    }

    // ---- Writes: see `shell/writes.rs` ----------------------------------------

    async fn insert(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
    ) -> Result<(String, Record<CborValue>)> {
        self.insert_row(id, record).await
    }

    async fn replace(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.replace_row(id, record).await
    }

    async fn upsert(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.upsert_row(id, record).await
    }

    async fn patch(&self, id: &str, partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.patch_row(id, partial).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.delete_row(id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.delete_rows().await
    }

    // ---- Live subscription ----------------------------------------------------

    /// Follows the Dio's event bus, re-evaluated through this handle's
    /// narrowing; see `dio/shell/watch.rs`.
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.follow().await
    }

    // ---- Capability and identity ----------------------------------------------

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "dio"
    }

    /// See `shell/preview.rs`.
    fn preview(&self) -> Preview {
        self.planned_preview()
    }
}

impl DioShell {
    /// The rows this handle sees: read where [`DioShell::plan`] routes them,
    /// then kept only when they satisfy the clauses the source didn't apply
    /// and the Dio's residual, and sorted when the source didn't take the
    /// order.
    ///
    /// When those clauses need augmented values
    /// ([`needs_hydration`](DioShell::needs_hydration)), every cached row
    /// still missing them is hydrated first, blocking until the detail pass
    /// settles: judging a gap would drop the row silently. A hydration
    /// failure is the read's error.
    pub(crate) async fn read(&self) -> Result<Rows> {
        let ReadPlan { source, local } = self.plan()?;
        let from_master = source.is_some();
        let mut rows = match source {
            Some(master) => master.list_values().await?,
            None => self.dio.cache.list_values().await?,
        };
        if self.needs_hydration(&local) {
            self.hydrate(&mut rows).await?;
        }
        self.refine(rows, &local, from_master)
    }

    /// The cached row `id` when this handle's query and the Dio's residual
    /// admit it. The row is hydrated first when `hydrate` is set or when
    /// judging it needs augmented values, so a gap never decides.
    pub(crate) async fn cached_in_view(
        &self,
        id: &str,
        hydrate: bool,
    ) -> Result<Option<Record<CborValue>>> {
        let Some(row) = self.dio.cache.get_value(id).await? else {
            return Ok(None);
        };
        let mut rows = IndexMap::from([(id.to_string(), row)]);
        if hydrate || self.needs_hydration(&self.query) {
            self.hydrate(&mut rows).await?;
        }
        Ok(self.refine(rows, &self.query, false)?.shift_remove(id))
    }

    /// Whether this handle's rows are exactly the cache's: neither the
    /// handle nor the Dio's residual narrows (an order alone doesn't).
    pub(crate) fn reads_whole_cache(&self) -> bool {
        !self.narrows() && !self.dio.residual_narrows()
    }

    /// `rows`, read from the master when `from_master` and from the cache
    /// otherwise, narrowed by `local` and the Dio's residual. `local` holding
    /// a clause the evaluator can't judge (a native condition) is an error,
    /// never a pass.
    ///
    /// Sorted by `local`'s order, or, when the handle has none, by the Dio's
    /// own: the residual's, which the master never applied, or on a cache
    /// read the Dio's whole query's, since the cache keeps no order. The
    /// sort is the evaluator's, which can differ from the master's collation
    /// (see [`Dio::cached`](crate::Dio::cached)).
    fn refine(&self, rows: Rows, local: &VistaQuery, from_master: bool) -> Result<Rows> {
        let mut kept = Vec::with_capacity(rows.len());
        for (id, row) in rows {
            if local.matches(&row, &self.metadata)? && admitted_by_dio(&self.dio, &row) {
                kept.push((id, row));
            }
        }
        let dio_order = if from_master {
            &self.dio.residual.order
        } else {
            &self.dio.query.order
        };
        match (&local.order, dio_order) {
            (None, Some((column, dir))) => VistaQuery::new()
                .with_order(column, *dir)
                .sort_rows(&mut kept, &self.metadata),
            _ => local.sort_rows(&mut kept, &self.metadata),
        }
        Ok(kept.into_iter().collect())
    }

    /// Facade reads hydrate what they return: any row still missing its
    /// augment columns runs the Dio's detail pass before the read resolves.
    /// No augmentations configured: a no-op.
    async fn hydrate(&self, rows: &mut Rows) -> Result<()> {
        self.dio.hydrate(rows).await
    }
}
