//! Bulk import — one entry point that picks the master's native path or
//! the per-record optimistic fallback.
//!
//! The split of responsibilities: [`VistaCapabilities::can_import`]
//! answers *whether the master can take the whole set in one operation*
//! (SQL COPY, Surreal batch insert — all-or-nothing by contract);
//! [`Dio::import_values`] answers *how these records get in regardless* —
//! native when advertised, otherwise record by record through the same
//! optimistic flash path a form save uses, where partial progress is
//! honest and reportable.
//!
//! [`VistaCapabilities::can_import`]: vantage_vista::VistaCapabilities::can_import

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet as _;
use vantage_types::Record;

use crate::dio::{Dio, DioEvent};

impl Dio {
    /// Store `records` (id → record) in the master.
    ///
    /// When the master advertises `can_import`, the whole set goes down
    /// in one driver-native operation, the cache absorbs the records,
    /// and sceneries hear a single
    /// [`DatasetChanged`](DioEvent::DatasetChanged). Otherwise each
    /// record runs through [`flash_insert`](Dio::flash_insert) — views
    /// see rows land one by one, exactly as if a user had entered them.
    ///
    /// `progress(done, total)` fires after every **completed row** —
    /// including a row skipped because its id was already there, which
    /// counts toward `done` but not toward the result — so a progress
    /// bar tracks the walk through the file rather than the write count.
    /// The native path reports once, with the full count. The fallback
    /// **stops at the first failure**: the error names the failing row
    /// and id, and `progress` has already reported how far the walk got
    /// — an import that stops at row 3,000 says so precisely, it never
    /// half-lands silently.
    ///
    /// Returns the number of records actually inserted. On the fallback
    /// path an id the master already holds is skipped — it still counts
    /// toward `progress`, never toward the result — so a re-run of the
    /// same set reports zero rather than claiming the set again.
    ///
    /// The skip is decided by a read before the write, so the count is
    /// exact only against a table nobody else is writing: a racing
    /// writer that creates one of these ids in between has its record
    /// kept (the driver's insert is idempotent — nothing is
    /// overwritten), but this import counts that row as its own. The
    /// count is a report for a person, and no data turns on it; making
    /// it exact needs an insert-if-absent the driver contract does not
    /// have today.
    pub async fn import_values(
        &self,
        records: IndexMap<String, Record<CborValue>>,
        mut progress: impl FnMut(usize, usize) + Send,
    ) -> Result<usize> {
        let total = records.len();
        let master = self.master();

        if master.capabilities().can_import {
            let stored = master.import_values(&records).await?;
            // The master holds the set now; make the cache agree and
            // announce membership moved — once, not per row.
            for (id, record) in &records {
                self.cache().insert_value(id, record).await?;
            }
            let _ = self.inner.event_bus.send(DioEvent::DatasetChanged);
            progress(total, total);
            return Ok(stored);
        }

        let stopped_at = |index: usize, id: &str, e: vantage_core::VantageError| {
            error!(
                format!(
                    "import stopped at row {} of {} (id '{}'): {}",
                    index + 1,
                    total,
                    id,
                    e
                ),
                row = index + 1,
                id = id.to_string(),
                detail = e.to_string()
            )
        };
        let mut inserted = 0;
        for (index, (id, record)) in records.iter().enumerate() {
            // A driver's insert is idempotent — an existing id comes back
            // as the stored record, not an error — so the count would
            // otherwise claim every row landed. Ask first; an id already
            // present is skipped and not counted.
            let exists = master
                .get_value(id)
                .await
                .map_err(|e| stopped_at(index, id, e))?
                .is_some();
            if !exists {
                self.flash_insert(id.clone(), record.clone())
                    .await
                    .map_err(|e| stopped_at(index, id, e))?;
                inserted += 1;
            }
            progress(index + 1, total);
        }
        Ok(inserted)
    }
}
