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

use std::ops::ControlFlow;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Context, Result, error};
use vantage_dataset::traits::ReadableValueSet as _;
use vantage_types::Record;

use crate::dio::{Dio, DioEvent};

/// What an import did — the numbers a caller reports to a person.
///
/// Separate fields rather than one count because the difference matters
/// to whoever reads it: "imported 0 of 500" reads like a failure, while
/// "0 imported, 500 already there" is the expected answer to importing
/// the same file twice, and a caller cannot derive the second from the
/// first (a retry after a partial import would get it wrong).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportOutcome {
    /// Records this import created.
    pub inserted: usize,
    /// Rows whose id the table already held. Their records were left
    /// exactly as they were — an import never overwrites.
    pub skipped: usize,
    /// The caller stopped the walk (see the `progress` callback);
    /// `inserted + skipped` is then how far it got, not the whole set.
    pub cancelled: bool,
}

impl ImportOutcome {
    /// Rows the walk reached, landed or skipped.
    pub fn processed(&self) -> usize {
        self.inserted + self.skipped
    }
}

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
    /// including a row skipped because its id was already there — so a
    /// progress bar tracks the walk through the set rather than the
    /// write count. Returning [`ControlFlow::Break`] stops the walk
    /// before the next row: that is the **only** way to interrupt an
    /// import, and it is why the callback is called per row rather than
    /// per write. What already landed stays landed (each row is its own
    /// write), and the outcome says `cancelled`. The native path is
    /// atomic by contract, so it reports once, at the end, and cannot be
    /// interrupted.
    ///
    /// The fallback **stops at the first failure**: the error names the
    /// failing row and id, and `progress` has already reported how far
    /// the walk got — an import that stops at row 3,000 says so
    /// precisely, it never half-lands silently.
    ///
    /// An id the master already holds is skipped, on either path (the
    /// native contract requires the driver to count only what it newly
    /// inserted). So a re-run of the same set reports zero inserted
    /// rather than claiming the set again.
    ///
    /// The skip is decided by a read before the write, so the counts are
    /// exact only against a table nobody else is writing: a racing
    /// writer that creates one of these ids in between is counted by this
    /// import as its own. The driver's insert is insert-if-absent by
    /// contract, so a racing writer's row is kept; the counts are a
    /// report for a person.
    pub async fn import_values(
        &self,
        records: IndexMap<String, Record<CborValue>>,
        mut progress: impl FnMut(usize, usize) -> ControlFlow<()> + Send,
    ) -> Result<ImportOutcome> {
        let total = records.len();
        let master = self.master();

        if master.capabilities().can_import {
            let inserted = master.import_values(&records).await?;
            // The master holds the set now; make the cache agree and
            // announce membership moved — once, not per row.
            for (id, record) in &records {
                self.cache().insert_value(id, record).await?;
            }
            let _ = self.inner.event_bus.send(DioEvent::DatasetChanged);
            let _ = progress(total, total);
            return Ok(ImportOutcome {
                inserted,
                skipped: total.saturating_sub(inserted),
                cancelled: false,
            });
        }

        let stopped_at = |index: usize, id: &str| {
            error!(
                "import stopped",
                row = index + 1,
                of = total,
                id = id.to_string()
            )
        };
        let mut outcome = ImportOutcome::default();
        for (index, (id, record)) in records.iter().enumerate() {
            // A driver's insert is insert-if-absent — an existing id comes back
            // as the stored record, not an error — so the count would
            // otherwise claim every row landed. Ask first; an id already
            // present is skipped and not counted.
            let exists = master
                .get_value(id)
                .await
                .with_context(|| stopped_at(index, id))?
                .is_some();
            if exists {
                outcome.skipped += 1;
            } else {
                self.flash_insert(id.clone(), record.clone())
                    .await
                    .with_context(|| stopped_at(index, id))?;
                outcome.inserted += 1;
            }
            if progress(index + 1, total).is_break() {
                outcome.cancelled = true;
                break;
            }
        }
        Ok(outcome)
    }
}
