use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{Column, Reference, SortDirection, TableShell, Vista, VistaCapabilities};

use crate::dio::shell::DioShell;
use crate::ops::ChangeFlash;

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

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(ChangeFlash::insert(id.clone(), record.clone()))
            .await?;
        Ok(with_injected_id(record, id))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(ChangeFlash::replace(id.clone(), record.clone()))
            .await?;
        Ok(with_injected_id(record, id))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(ChangeFlash::new(
            crate::ops::FlashKind::Patch,
            Some(id.clone()),
            partial.clone(),
        ))
        .await?;
        Ok(with_injected_id(partial, id))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.enqueue(ChangeFlash::delete(id.clone())).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.enqueue(ChangeFlash::clear()).await
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
    async fn read(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let (narrowed, local) = self.plan();
        let mut rows = match narrowed {
            Some(master) => master.list_values().await?,
            None => self.dio.cache.list_values().await?,
        };

        rows.retain(|_, row| {
            local
                .conditions
                .iter()
                .all(|(field, expected)| record_get(row, field) == Some(expected))
        });
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
fn record_get<'a>(record: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
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
