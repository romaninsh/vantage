//! [`Servo::flash`]: actuating the servo.

use std::sync::atomic::Ordering;

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::{Record, cbor_id_to_string};

use super::{IdStrategy, Servo, ServoStatus, mint_generated, tracking};
use crate::ops::{ChangeFlash, FlashOutcome, FlashRejection};

impl Servo {
    /// Actuate: freeze the error signal into an immutable [`ChangeFlash`] and
    /// write it through [`Dio::flash`](crate::Dio::flash). Only the changed
    /// fields travel. Returns `Ok(None)` when the error is zero — nothing
    /// dirty, nothing fired — and otherwise the flash as it settled, under
    /// the id its row settled under.
    ///
    /// The diff is taken synchronously at the moment of the call; the
    /// emitted flash never changes afterwards. The servo stays a **draft**
    /// for the whole write: setpoints hold locked and the status reports
    /// [`Pending`](ServoStatus::Pending). Confirmation absorbs the confirmed
    /// record — convergence zeroes the error and releases every lock.
    /// Rejection absorbs the restored pre-image and the setpoints still
    /// stand: the user's values survive, dirty, with the failure in
    /// [`Failed`](ServoStatus::Failed). A write outside the Dio's set is
    /// rejected the same way, as `Conflict`.
    ///
    /// On a servo without a baseline the flash is an insert of the full
    /// record; the id comes from the servo's binding (minted at creation for
    /// [`IdStrategy::Uuid`]), from the record's id column
    /// ([`IdStrategy::FromRecord`]), or from the backend
    /// ([`IdStrategy::Auto`], through [`ChangeFlash::insert_new`]).
    pub async fn flash(&self) -> Result<Option<ChangeFlash>> {
        let Some(flash) = self.freeze()? else {
            return Ok(None);
        };

        // Draft semantics: bind identity and report Pending, but neither the
        // baseline nor the setpoints move until the write resolves.
        {
            *self.state.id.write().unwrap() = flash.id().map(str::to_string);
            *self.state.status.write().unwrap() = ServoStatus::Pending;
        }
        self.state.in_flight.fetch_add(1, Ordering::SeqCst);
        self.state.bump_generation();

        let outcome = self.dio.flash(flash.clone()).await;
        // The backend may keep the row under an id of its own (or make one,
        // for an id-less insert), so identity comes from the id the row
        // settled under. Rebound before the measurement below, which reads
        // the cache by this servo's id.
        if let Ok(FlashOutcome {
            id: Some(settled), ..
        }) = &outcome
        {
            *self.state.id.write().unwrap() = Some(settled.clone());
        }
        // One deliberate measurement now that the write resolved — but only
        // for the LAST in-flight resolver: an earlier one would read a
        // sibling flash's still-staged value as an upstream measurement,
        // exactly what the in-flight gate exists to prevent. On success: the
        // confirmed value (convergence releases every lock). On failure: the
        // restored pre-image (setpoints still held — the draft survives).
        let last = self.state.in_flight.fetch_sub(1, Ordering::SeqCst) == 1;
        let stored = outcome.as_ref().ok().and_then(|o| o.record.clone());
        if last {
            self.absorb_now(stored).await;
        }
        match outcome {
            Ok(outcome) => {
                if last {
                    self.state.set_status(ServoStatus::Tracking);
                }
                Ok(Some(settled_flash(flash, outcome)))
            }
            Err(e) => {
                // The failure is reported regardless — a sibling's later
                // success overwrites the status, which is the honest
                // last-write-wins reading of overlapping saves.
                self.state
                    .set_status(ServoStatus::Failed(FlashRejection::from_error_or_message(
                        &e,
                    )));
                Err(e)
            }
        }
    }

    /// The flash the current error signal makes, or `None` when there is
    /// nothing to write.
    fn freeze(&self) -> Result<Option<ChangeFlash>> {
        let baseline = self.state.baseline.read().unwrap();
        let data = self.state.data.read().unwrap();
        if let Some(base) = baseline.as_ref() {
            let error = tracking::error_of(Some(base), &data);
            if error.is_empty() {
                return Ok(None);
            }
            let Some(id) = self.state.id.read().unwrap().clone() else {
                return Err(error!(
                    "a servo with a baseline has no bound id to patch",
                    table = self.dio.master().name()
                ));
            };
            return Ok(Some(
                ChangeFlash::partial(id, error).with_before(base.clone()),
            ));
        }
        if data.is_empty() {
            return Ok(None);
        }
        let flash = match self.state.id.read().unwrap().clone() {
            // Identity lives in the binding, not the draft: the insert record
            // carries the id column even though the user never typed it.
            Some(id) => {
                let mut record = data.clone();
                record
                    .entry(self.dio.id_column().to_string())
                    .or_insert_with(|| CborValue::Text(id.clone()));
                ChangeFlash::insert(id, record)
            }
            None if self.state.strategy == IdStrategy::Auto => {
                ChangeFlash::insert_new(data.clone())
            }
            // The generator failed when the servo opened: mint now, before
            // this first attempt, or fail the flash with its error. `flash`
            // binds the id, so a retry reuses it.
            None if self.state.strategy == IdStrategy::Generated => {
                let id = mint_generated(&self.dio).map_err(|e| {
                    error!(
                        "couldn't mint an id for the new record",
                        table = self.dio.master().name(),
                        error = e.to_string()
                    )
                })?;
                let mut record = data.clone();
                record
                    .entry(self.dio.id_column().to_string())
                    .or_insert_with(|| CborValue::Text(id.clone()));
                ChangeFlash::insert(id, record)
            }
            None => ChangeFlash::insert(self.id_from_record(&data)?, data.clone()),
        };
        Ok(Some(flash))
    }

    /// Take a measurement from the cache right now — the deliberate
    /// post-resolution read `flash` performs (the bus task holds absorbs off
    /// while our own write is in flight). `stored`, the row the write
    /// reported, stands in when the cache doesn't hold it, so a servo whose
    /// new row wasn't seeded still tracks it and patches it next time
    /// rather than inserting it again.
    async fn absorb_now(&self, stored: Option<Record<CborValue>>) {
        let Some(id) = self.state.id.read().unwrap().clone() else {
            return;
        };
        let inner = &self.dio.inner;
        match inner.read_admitted(inner.cache.as_ref(), &id).await {
            Ok(value) => self.state.absorb(value.or(stored)),
            Err(e) => tracing::error!(error = %e, "servo measurement read failed"),
        }
    }

    /// Resolve an insert id from the record's id column.
    fn id_from_record(&self, data: &Record<CborValue>) -> Result<String> {
        let id_column = self.dio.id_column();
        let id = data
            .get(id_column)
            .and_then(cbor_id_to_string)
            .filter(|s| !s.is_empty());
        id.ok_or_else(|| {
            error!(
                "flashing a new record requires its id field",
                id_column = id_column
            )
        })
    }
}

/// `flash` as it settled: re-bound to the id its row settled under, and for
/// an id-less insert, the row as stored.
fn settled_flash(mut flash: ChangeFlash, outcome: FlashOutcome) -> ChangeFlash {
    match (flash.id().is_some(), outcome.id) {
        (true, Some(settled)) => {
            flash.rebind_id(settled);
            flash
        }
        (false, Some(id)) => {
            ChangeFlash::insert(id, outcome.record.unwrap_or_else(|| flash.patch().clone()))
        }
        _ => flash,
    }
}
