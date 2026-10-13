//! Tracking upstream: the servo-loop arithmetic, and the bus task that feeds
//! it measurements.
//!
//! The vocabulary is a servomechanism's: `data` holds the commanded
//! setpoints, `baseline` the measured upstream state, and the **error
//! signal** is their per-field difference. A field with zero error is in
//! *continuous tracking* (it follows the measurement); a field with a
//! commanded setpoint is *locked* (it holds until actuated or released).

use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};

use ciborium::Value as CborValue;
use tokio::sync::broadcast;
use vantage_types::Record;

use super::{ServoState, ServoStatus};
use crate::dio::{DioEvent, DioInner};
use crate::ops::{FlashKind, FlashRejection};

/// The error signal: every field of `data` whose value differs from the
/// baseline. With no baseline (a record that doesn't exist yet), every
/// commanded field is error.
pub(crate) fn error_of(
    baseline: Option<&Record<CborValue>>,
    data: &Record<CborValue>,
) -> Record<CborValue> {
    let mut error = Record::new();
    for (k, v) in data {
        if baseline.and_then(|b| b.get(k)) != Some(v) {
            error.insert(k.clone(), v.clone());
        }
    }
    error
}

/// Absorb a new upstream measurement.
///
/// Per field: zero error (tracking) → adopt the incoming value, including
/// adoption of new fields and removal of fields the upstream dropped;
/// non-zero error (locked) → hold the setpoint, only the baseline moves.
/// A field whose incoming measurement equals its setpoint has, by
/// definition, zero error afterwards — convergence releases the lock with
/// no special case.
///
/// `incoming: None` means the record vanished upstream: the baseline
/// clears (every remaining setpoint becomes error), the setpoints stay.
pub(crate) fn absorb(
    baseline: &mut Option<Record<CborValue>>,
    data: &mut Record<CborValue>,
    incoming: Option<Record<CborValue>>,
) {
    let Some(inc) = incoming else {
        *baseline = None;
        return;
    };

    let old = baseline.take();
    let mut next = Record::new();

    // Fields the measurement carries: adopt when tracking, hold when locked.
    for (k, measured) in &inc {
        let was = old.as_ref().and_then(|b| b.get(k));
        match data.get(k) {
            Some(setpoint) if Some(setpoint) != was => {
                next.insert(k.clone(), setpoint.clone());
            }
            _ => {
                next.insert(k.clone(), measured.clone());
            }
        }
    }

    // Fields we hold that the measurement doesn't carry: a locked field
    // (a commanded addition) survives; a tracking field follows the
    // upstream removal and drops.
    for (k, setpoint) in data.iter() {
        if inc.get(k).is_some() {
            continue;
        }
        if old.as_ref().and_then(|b| b.get(k)) != Some(setpoint) {
            next.insert(k.clone(), setpoint.clone());
        }
    }

    *data = next;
    *baseline = Some(inc);
}

/// The bus-tracking loop: every event about the bound record feeds the
/// measurement side of the loop from the cache. While this servo's own
/// flash is in flight, everything is held off — `flash` takes its own
/// measurement on resolution, so the optimistic stage of our own write
/// never reads as upstream truth.
pub(super) async fn track_loop(
    state: Arc<ServoState>,
    dio_weak: Weak<DioInner>,
    mut bus: broadcast::Receiver<DioEvent>,
) {
    loop {
        if dio_weak.upgrade().is_none() {
            return;
        }
        let event = match bus.recv().await {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Missed events: re-measure from the cache.
                if state.in_flight.load(Ordering::SeqCst) == 0 {
                    absorb_from_cache(&state, &dio_weak).await;
                }
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => return,
        };
        if state.in_flight.load(Ordering::SeqCst) > 0 {
            continue;
        }
        let bound = |id: &str| state.id.read().unwrap().as_deref() == Some(id);
        // Only editing kinds are this servo's business: a staged or reverted
        // Delete/Clear belongs to its issuer (the confirm dialog), and a form
        // displaying the record must not adopt it as its own write.
        let editing = |kind: FlashKind| !matches!(kind, FlashKind::Delete | FlashKind::Clear);
        match event {
            DioEvent::RecordChanged { id }
            | DioEvent::RecordInserted { id }
            | DioEvent::RecordRemoved { id }
                if bound(&id) =>
            {
                absorb_from_cache(&state, &dio_weak).await;
            }
            // `Seeded` alongside `DatasetChanged`: a servo bound before the
            // eager load finished has nothing to absorb until it lands.
            DioEvent::DatasetChanged | DioEvent::Seeded => {
                absorb_from_cache(&state, &dio_weak).await;
            }
            DioEvent::WritePending { id, kind } if bound(&id) && editing(kind) => {
                state.set_status(ServoStatus::Pending);
            }
            DioEvent::WriteReverted { id, error, kind } if bound(&id) => {
                if editing(kind) {
                    state.set_status(ServoStatus::Failed(FlashRejection::new(error)));
                }
                absorb_from_cache(&state, &dio_weak).await;
            }
            _ => {}
        }
    }
}

/// Measure the bound record from the cache: the row the Dio's residual
/// admits, or none.
async fn absorb_from_cache(state: &Arc<ServoState>, dio_weak: &Weak<DioInner>) {
    let Some(inner) = dio_weak.upgrade() else {
        return;
    };
    let Some(id) = state.id.read().unwrap().clone() else {
        return;
    };
    match inner.read_admitted(inner.cache.as_ref(), &id).await {
        Ok(value) => state.absorb(value),
        Err(e) => tracing::error!(error = %e, "servo measurement read failed"),
    }
}

#[cfg(test)]
mod tests;
