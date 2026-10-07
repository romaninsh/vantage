use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{
    Column, Reference, SortDirection, TableShell, Vista, VistaCapabilities, VistaChangeStream,
};

use crate::dio::shell::DioShell;
use crate::ops::{ChangeFlash, FlashKind};

#[async_trait]
impl TableShell for DioShell {
    // ---- Schema forwarding to master ----------------------------------------

    fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        &self.references
    }

    fn id_column(&self) -> Option<&str> {
        self.id_column.as_deref()
    }

    // ---- Reads — cache-first; bounded reads hydrate ---------------------------
    //
    // When the Dio carries augmentations, a BOUNDED facade read
    // (`get_value`, `fetch_window`) runs the detail pass for every
    // returned row that still has an augment gap — the read blocks until
    // its rows are fully hydrated (and cached, so the cost is paid once).
    // `list_values` deliberately stays cheap: a listing is the fast spine,
    // and hydrating an entire set through it means one innocent-looking
    // call downloading everything. Ask for a window when you want details.

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.read().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        // A narrowed handle is a smaller set: an id outside it is not found,
        // rather than found and silently outside the filter.
        let Some(row) = (if self.query.is_empty() {
            self.dio.cache.get_value(id).await?
        } else {
            self.read().await?.shift_remove(id)
        }) else {
            return Ok(None);
        };
        let mut rows = IndexMap::from([(id.clone(), row)]);
        self.hydrate(&mut rows).await?;
        Ok(rows.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let rows = self.read().await?;
        let Some((id, row)) = rows.into_iter().next() else {
            return Ok(None);
        };
        let mut one = IndexMap::from([(id.clone(), row)]);
        self.hydrate(&mut one).await?;
        Ok(one.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        if self.query.is_empty() {
            return self.dio.cache.count().await;
        }
        Ok(self.read().await?.len() as i64)
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        let all = self.read().await?;
        let mut rows: IndexMap<String, Record<CborValue>> =
            all.into_iter().skip(offset).take(limit).collect();
        self.hydrate(&mut rows).await?;
        Ok(rows.into_iter().collect())
    }

    // ---- Narrowing -----------------------------------------------------------
    //
    // Recorded here and routed at read time (see `DioShell::plan`): pushed into
    // the master when it can answer them, applied over the cache when it
    // cannot. Accrete/replace semantics match `Vista`'s: conditions accumulate,
    // order replaces.

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.query
            .conditions
            .push((field.to_string(), value.clone()));
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        self.query.order = Some((field.to_string(), dir));
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.query.order = None;
        Ok(())
    }

    /// Clones carry the narrowing but stay independent, so narrowing a derived
    /// handle never disturbs the one it came from.
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(DioShell {
            dio: self.dio.clone(),
            capabilities: self.capabilities.clone(),
            columns: self.columns.clone(),
            references: self.references.clone(),
            id_column: self.id_column.clone(),
            query: self.query.clone(),
        }))
    }

    // ---- Writes — enqueue + return synthesized record ------------------------
    //
    // Writes are fire-and-forget: the queue accepts the op and returns
    // immediately. Failures land on the event bus as
    // `DioEvent::WriteFailed`. The synthesized record echoes the input
    // (with the id injected) — callers that need authoritative
    // server-side data should refetch via `get_value` after the write
    // completes (an `on_flash` route typically updates the cache too).

    /// Insert-if-absent: an id already in view returns the stored row and
    /// queues nothing; an id held outside the narrowing is `Conflict`.
    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        if let Some(row) = self.row_in_view_or_conflict(id).await? {
            return Ok(row);
        }
        self.enqueue_full_record(FlashKind::Insert, id, &self.conformed(record)?)
            .await
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.row_in_view_or_conflict(id).await?;
        self.enqueue_full_record(FlashKind::Replace, id, &self.conformed(record)?)
            .await
    }

    async fn upsert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.row_in_view_or_conflict(id).await?;
        self.enqueue_full_record(FlashKind::Upsert, id, &self.conformed(record)?)
            .await
    }

    /// The write queue is fire-and-forget, so a missing row is caught here
    /// rather than surfacing later as `DioEvent::WriteFailed`.
    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        if self.row_in_view(id).await?.is_none() {
            return Err(
                error!("Row not found", table = self.dio.cache_table_name, id = id)
                    .mark_not_found(),
            );
        }
        vantage_dataset::invariants::validate(partial, &self.query.invariants())?;
        self.enqueue(ChangeFlash::new(
            crate::ops::FlashKind::Patch,
            Some(id.clone()),
            partial.clone(),
        ))
        .await?;
        Ok(with_injected_id(partial, id))
    }

    /// Idempotent: a row missing or outside the narrowing is already gone
    /// from this set, so nothing is queued and the call succeeds.
    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        if self.row_in_view(id).await?.is_some() {
            self.enqueue(ChangeFlash::delete(id.clone())).await?;
        }
        Ok(())
    }

    /// An unnarrowed handle clears the master. A narrowed one is a subset:
    /// it deletes exactly the rows it reads, one `Delete` flash each, and
    /// never sends `Clear`. Routes and the default write path read `Clear`
    /// as "every row".
    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        if self.query.conditions.is_empty() {
            return self.enqueue(ChangeFlash::clear()).await;
        }
        for id in self.read().await?.into_keys() {
            self.enqueue(ChangeFlash::delete(id)).await?;
        }
        Ok(())
    }

    /// No id exists until the master assigns one — see
    /// `Dio::insert_returning_id`, which
    /// bypasses the write queue and any `on_flash` route and seeds the
    /// cache, so the id returned here is visible to an immediate
    /// `get_value`.
    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let dio = crate::Dio {
            inner: self.dio.clone(),
        };
        let (id, _stored) = dio.insert_returning_id(&self.conformed(record)?).await?;
        Ok(id)
    }

    // ---- Live subscription ------------------------------------------------------

    /// Follows the Dio's event bus, re-evaluated through this handle's
    /// narrowing — see `dio/shell/watch.rs`.
    async fn watch_vista(&self, _vista: &Vista) -> Result<VistaChangeStream> {
        self.follow().await
    }

    // ---- Capability + identity ------------------------------------------------

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "dio"
    }

    /// A facade read hits the local cache, not a backend — so the query worth
    /// seeing is the **master's**, which is what filled that cache in the first
    /// place.
    ///
    /// Which clauses that master carries is not a given: `plan` offers each one
    /// to a private clone and keeps whatever it accepts, so the same condition
    /// is a server-side filter against one driver and an in-memory one against
    /// the next. The preview therefore reports the
    /// **planned** master, and lists under `facade` only what the master
    /// refused. Reporting the bare master and every clause as local would put
    /// pushed-down filters in the wrong column — lossy is acceptable here,
    /// wrong is not.
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        let (planned, local) = self.plan();
        let master = match &planned {
            Some(narrowed) => narrowed.preview_query(),
            // Nothing pushed down: the master runs unnarrowed.
            None => self.dio.master.read().unwrap().preview_query(),
        };

        let conditions: Vec<String> = local
            .conditions
            .iter()
            .map(|(field, value)| format!("{field} = {value:?}"))
            .collect();
        let order = local.order.as_ref().map(|(col, dir)| {
            let dir = match dir {
                SortDirection::Ascending => "asc",
                SortDirection::Descending => "desc",
            };
            format!("{col} {dir}")
        });

        serde_json::json!({
            "driver": "dio",
            "note": "reads come from the local cache; this issues no query of \
                     its own. `master` is the query that populates that cache, \
                     carrying every clause the driver accepted; `facade` is what \
                     it refused, applied to the cached rows in memory.",
            "master": master,
            "facade": { "conditions": conditions, "order": order },
        })
    }
}

impl DioShell {
    /// The rows this handle sees, narrowing included.
    ///
    /// An unnarrowed handle reads the cache, exactly as before. A narrowed one
    /// asks the master for whatever the master can answer — that result is
    /// authoritative over the whole set, not over whatever the cache happens to
    /// hold — and applies the rest here. See [`DioShell::plan`] for the routing.
    pub(crate) async fn read(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let (narrowed, local) = self.plan();
        let mut rows = match narrowed {
            Some(master) => master.list_values().await?,
            None => self.dio.cache.list_values().await?,
        };

        rows.retain(|_, row| local.matches(row));
        if let Some((column, direction)) = &local.order {
            let descending = matches!(direction, SortDirection::Descending);
            let mut ordered: Vec<(String, Record<CborValue>)> = rows.into_iter().collect();
            ordered.sort_by(|(a_id, a), (b_id, b)| {
                // Absent values last in both directions, and ties broken on id
                // so the same rows always come back in the same order.
                let ordering = match (record_get(a, column), record_get(b, column)) {
                    (None, None) => std::cmp::Ordering::Equal,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (Some(left), Some(right)) => {
                        let ordering = cbor_cmp(left, right);
                        if descending {
                            ordering.reverse()
                        } else {
                            ordering
                        }
                    }
                };
                ordering.then_with(|| a_id.cmp(b_id))
            });
            rows = ordered.into_iter().collect();
        }
        Ok(rows)
    }

    /// Facade reads hydrate what they return: any row still missing its
    /// augment columns runs the Dio's detail pass before the read
    /// resolves. No augmentations configured → no-op.
    async fn hydrate(&self, rows: &mut IndexMap<String, Record<CborValue>>) -> Result<()> {
        if !self.dio.has_dio_augment() {
            return Ok(());
        }
        let dio = crate::Dio {
            inner: self.dio.clone(),
        };
        crate::dio::augment_passes::hydrate_gaps(&dio, rows).await
    }

    /// The row with `id`, ignoring the narrowing — one point read, never a
    /// listing. An unnarrowed handle has no set to protect: it reads the
    /// cache, then the master on a miss, so a cache hit never touches the
    /// master. A narrowed handle reads the master only, because a stale
    /// cached row must not decide whether a write stays inside the set.
    async fn row_anywhere(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        if self.query.conditions.is_empty()
            && let Some(row) = self.dio.cache.get_value(id).await?
        {
            return Ok(Some(row));
        }
        let master = self.dio.master.read().unwrap().clone();
        master.get_value(id).await
    }

    /// The row `id` as this handle sees it: `row_anywhere`, kept only when
    /// it satisfies the narrowing.
    async fn row_in_view(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self
            .row_anywhere(id)
            .await?
            .filter(|row| self.query.matches(row)))
    }

    /// Like `row_in_view`, but an id held by a row outside the narrowing is
    /// `Conflict`: a full-record write may create the row, never take over
    /// one outside the set.
    async fn row_in_view_or_conflict(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        match self.row_anywhere(id).await? {
            Some(row) if !self.query.matches(&row) => Err(self.outside(id)),
            row => Ok(row),
        }
    }

    fn outside(&self, id: &str) -> vantage_core::VantageError {
        error!(
            "id is held by a row outside this set",
            table = self.dio.cache_table_name,
            id = id
        )
        .mark_conflict()
    }

    /// Fill and check the handle's equality conditions.
    fn conformed(&self, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        let mut out = record.clone();
        vantage_dataset::invariants::conform(&mut out, &self.query.invariants())?;
        Ok(out)
    }

    /// `insert`/`replace`/`upsert` share one shape: enqueue the whole
    /// record under `id` as the given flash kind, then hand back the
    /// synthesized record — see the module doc above `insert_vista_value`.
    async fn enqueue_full_record(
        &self,
        kind: FlashKind,
        id: &str,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(ChangeFlash::new(kind, Some(id.to_string()), record.clone()))
            .await?;
        Ok(with_injected_id(record, id))
    }

    async fn enqueue(&self, flash: ChangeFlash) -> Result<()> {
        // The queued flash carries its own keep-alive: even if every
        // external handle drops right after this returns, the pipeline
        // stays alive until the write lands.
        self.dio
            .write_queue
            .send(crate::dio::worker::QueuedFlash {
                flash,
                keep_alive: self.dio.clone(),
            })
            .await
            .map_err(|e| error!("Dio write queue closed", detail = e.to_string()))
    }
}

/// Resolve a column, descending dotted paths into nested CBOR maps so a
/// belongs-to leaf (`client.name`) narrows like any other column.
pub(crate) fn record_get<'a>(record: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    if let Some(value) = record.get(path) {
        return Some(value);
    }
    let mut segments = path.split('.');
    let mut current = record.get(segments.next()?)?;
    for segment in segments {
        let CborValue::Map(entries) = current else {
            return None;
        };
        current = entries.iter().find_map(|(key, value)| match key {
            CborValue::Text(name) if name == segment => Some(value),
            _ => None,
        })?;
    }
    Some(current)
}

/// Numbers compare numerically across the integer/float boundary — never by
/// their debug rendering, which ranks `657.96` above `1826.19`. `NaN` compares
/// equal rather than panicking the sort.
fn cbor_cmp(a: &CborValue, b: &CborValue) -> std::cmp::Ordering {
    fn number(value: &CborValue) -> Option<f64> {
        match value {
            CborValue::Integer(i) => Some(i128::from(*i) as f64),
            CborValue::Float(f) => Some(*f),
            _ => None,
        }
    }
    match (a, b) {
        (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
        (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
        _ => match (number(a), number(b)) {
            (Some(l), Some(r)) => l.partial_cmp(&r).unwrap_or(std::cmp::Ordering::Equal),
            _ => std::cmp::Ordering::Equal,
        },
    }
}

fn with_injected_id(record: &Record<CborValue>, id: &str) -> Record<CborValue> {
    let mut out = record.clone();
    out.insert("id".to_string(), CborValue::Text(id.to_string()));
    out
}
