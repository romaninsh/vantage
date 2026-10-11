use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, Condition, FilterOp, Page, Preview, Reference, SortDirection, TableShell,
    Vista, VistaCapabilities, VistaChangeStream, unsupported,
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
    // (`get`, `fetch_window`) runs the detail pass for every
    // returned row that still has an augment gap — the read blocks until
    // its rows are fully hydrated (and cached, so the cost is paid once).
    // `list` deliberately stays cheap: a listing is the fast spine,
    // and hydrating an entire set through it means one innocent-looking
    // call downloading everything. Ask for a window when you want details.

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.read().await
    }

    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        // A narrowed handle is a smaller set: an id outside it is not found,
        // rather than found and silently outside the filter.
        let Some(row) = (if self.query.is_empty() {
            self.dio.cache.get_value(id).await?
        } else {
            self.read().await?.shift_remove(id)
        }) else {
            return Ok(None);
        };
        let mut rows = IndexMap::from([(id.to_string(), row)]);
        self.hydrate(&mut rows).await?;
        Ok(rows.shift_remove(id))
    }

    async fn count(&self) -> Result<i64> {
        if self.query.is_empty() {
            return self.dio.cache.count().await;
        }
        Ok(self.read().await?.len() as i64)
    }

    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let all = self.read().await?;
        let total = all.len() as i64;
        let mut rows: IndexMap<String, Record<CborValue>> =
            all.into_iter().skip(offset).take(limit).collect();
        self.hydrate(&mut rows).await?;
        Ok(Page::new(rows.into_iter().collect()).with_total(total))
    }

    // ---- Narrowing -----------------------------------------------------------
    //
    // Recorded here and routed at read time (see `DioShell::plan`): pushed into
    // the master when it can answer them, applied over the cache when it
    // cannot. Conditions accumulate, order replaces. A clone carries the
    // narrowing but owns it, so narrowing a derived handle never disturbs the
    // one it came from.

    /// Takes equality on a facade column, an augmented column, or a dotted
    /// path into a facade column; any other column is an error. A facade
    /// over a master with no declared columns can't check, and takes any.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        let column = condition.column.as_str();
        let root = column.split('.').next().unwrap_or(column);
        if !self.columns.is_empty()
            && !self.columns.contains_key(column)
            && !self.columns.contains_key(root)
            && !self.dio.is_augmented_column(column)
        {
            return Err(error!(
                "Unknown column",
                column = column,
                table = self.dio.cache_table_name
            ));
        }
        self.query
            .conditions
            .push((condition.column.clone(), condition.value.clone()));
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.query.order = order.map(|(field, dir)| (field.to_string(), dir));
        Ok(())
    }

    // ---- Writes — enqueue + return synthesized record ------------------------
    //
    // Writes are fire-and-forget: the queue accepts the op and returns
    // immediately. Failures land on the event bus as
    // `DioEvent::WriteFailed`. The synthesized record echoes the input
    // (with the id injected) — callers that need authoritative
    // server-side data should refetch via `get_value` after the write
    // completes (an `on_flash` route typically updates the cache too).

    /// With an id, insert-if-absent: an id already in view returns the stored
    /// row and queues nothing; an id held outside the narrowing is `Conflict`.
    ///
    /// Without one, no id exists until the master assigns it — see
    /// `Dio::insert_returning_id`, which bypasses the write queue and any
    /// `on_flash` route and seeds the cache, so the returned id is visible to
    /// an immediate `get_value`.
    async fn insert(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
    ) -> Result<(String, Record<CborValue>)> {
        let Some(id) = id else {
            let dio = crate::Dio {
                inner: self.dio.clone(),
            };
            return dio.insert_returning_id(&self.conformed(record)?).await;
        };
        if let Some(row) = self.row_in_view_or_conflict(id).await? {
            return Ok((id.to_string(), row));
        }
        let stored = self
            .enqueue_full_record(FlashKind::Insert, id, &self.conformed(record)?)
            .await?;
        Ok((id.to_string(), stored))
    }

    async fn replace(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.row_in_view_or_conflict(id).await?;
        self.enqueue_full_record(FlashKind::Replace, id, &self.conformed(record)?)
            .await
    }

    async fn upsert(&self, id: &str, record: &Record<CborValue>) -> Result<Record<CborValue>> {
        self.row_in_view_or_conflict(id).await?;
        self.enqueue_full_record(FlashKind::Upsert, id, &self.conformed(record)?)
            .await
    }

    /// The write queue is fire-and-forget, so a missing row is caught here
    /// rather than surfacing later as `DioEvent::WriteFailed`.
    async fn patch(&self, id: &str, partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        if self.row_in_view(id).await?.is_none() {
            return Err(
                error!("Row not found", table = self.dio.cache_table_name, id = id)
                    .mark_not_found(),
            );
        }
        vantage_dataset::invariants::validate(partial, &self.query.invariants())?;
        self.enqueue(ChangeFlash::new(
            FlashKind::Patch,
            Some(id.to_string()),
            partial.clone(),
        ))
        .await?;
        Ok(with_injected_id(partial, id))
    }

    /// Idempotent: a row missing or outside the narrowing is already gone
    /// from this set, so nothing is queued and the call succeeds.
    async fn delete(&self, id: &str) -> Result<()> {
        if self.row_in_view(id).await?.is_some() {
            self.enqueue(ChangeFlash::delete(id.to_string())).await?;
        }
        Ok(())
    }

    /// An unnarrowed handle clears the master. A narrowed one is a subset:
    /// it deletes exactly the rows it reads, one `Delete` flash each, and
    /// never sends `Clear`. Routes and the default write path read `Clear`
    /// as "every row".
    async fn delete_all(&self) -> Result<()> {
        if self.query.conditions.is_empty() {
            return self.enqueue(ChangeFlash::clear()).await;
        }
        for id in self.read().await?.into_keys() {
            self.enqueue(ChangeFlash::delete(id)).await?;
        }
        Ok(())
    }

    // ---- Live subscription ------------------------------------------------------

    /// Follows the Dio's event bus, re-evaluated through this handle's
    /// narrowing — see `dio/shell/watch.rs`.
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.follow().await
    }

    // ---- References ---------------------------------------------------------

    /// The facade doesn't traverse: a related set needs its own Dio, which
    /// `Dio::get_ref` builds.
    fn get_ref(&self, _relation: &str, _row: Option<&Record<CborValue>>) -> Result<Vista> {
        unsupported(Capability::Traverse)
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
    /// Which clauses that master carries is not a given: `plan` offers them
    /// to a clone and keeps whatever it accepts, so the same condition is a
    /// server-side filter against one driver and an in-memory one against the
    /// next. The preview therefore reports the **planned** master, and notes
    /// only what the master refused as applied over the cache. Reporting the
    /// bare master and every clause as local would put pushed-down filters in
    /// the wrong place — lossy is acceptable here, wrong is not.
    fn preview(&self) -> Preview {
        let (planned, local) = match self.plan() {
            Ok(plan) => plan,
            Err(e) => {
                return Preview::new("dio")
                    .with_note(format!("the master refused this handle's query: {e}"));
            }
        };
        let master = match &planned {
            Some(narrowed) => narrowed.preview(),
            // Nothing pushed down: the master runs unnarrowed.
            None => self.dio.master.read().unwrap().preview(),
        };

        let mut facade: Vec<String> = local
            .conditions
            .iter()
            .map(|(field, value)| format!("{field} = {value:?}"))
            .collect();
        if let Some((col, dir)) = &local.order {
            let dir = match dir {
                SortDirection::Ascending => "asc",
                SortDirection::Descending => "desc",
            };
            facade.push(format!("order by {col} {dir}"));
        }

        let mut note = format!(
            "reads come from the local cache; the query is the {} master's, \
             which populates that cache and carries every clause it accepted",
            master.driver
        );
        if !facade.is_empty() {
            note.push_str("; applied over the cache: ");
            note.push_str(&facade.join(", "));
        }
        let preview = Preview::new("dio").with_note(note);
        match master.query {
            Some(query) => preview.with_query(query),
            None => preview,
        }
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
        let (narrowed, local) = self.plan()?;
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
    /// synthesized record — see the "Writes" comment above `insert`.
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
