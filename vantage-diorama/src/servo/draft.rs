//! Reading and commanding the servo's draft: setpoints, the measurement and
//! the error signal between them.

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::{Servo, tracking};

impl Servo {
    /// Command a setpoint — the field locks and holds until the servo
    /// is actuated ([`flash`](Self::flash)), released
    /// ([`revert`](Self::revert)), or the measurement converges on it.
    pub fn set(&self, field: impl Into<String>, value: impl Into<CborValue>) {
        self.state
            .data
            .write()
            .unwrap()
            .insert(field.into(), value.into());
        self.state.bump_generation();
    }

    /// The current value of `field` — the setpoint when locked, the
    /// measurement when tracking. This is what a form renders.
    pub fn get(&self, field: &str) -> Option<CborValue> {
        self.state.data.read().unwrap().get(field).cloned()
    }

    /// The full record as currently displayed (setpoints over measurement).
    pub fn record(&self) -> Record<CborValue> {
        self.state.data.read().unwrap().clone()
    }

    /// The measured upstream state, `None` for a record that doesn't
    /// exist (yet).
    pub fn baseline(&self) -> Option<Record<CborValue>> {
        self.state.baseline.read().unwrap().clone()
    }

    /// The error signal: every field whose displayed value differs from
    /// the baseline. This — and only this — is what a flash will carry.
    pub fn error(&self) -> Record<CborValue> {
        let baseline = self.state.baseline.read().unwrap();
        let data = self.state.data.read().unwrap();
        tracking::error_of(baseline.as_ref(), &data)
    }

    /// Whether `field` currently carries error (is locked on a setpoint).
    pub fn dirty(&self, field: &str) -> bool {
        self.error().get(field).is_some()
    }

    /// Whether any field carries error.
    pub fn is_dirty(&self) -> bool {
        !self.error().is_empty()
    }

    /// Release one field back to tracking: its value returns to the
    /// baseline measurement.
    pub fn revert(&self, field: &str) {
        {
            let baseline = self.state.baseline.read().unwrap();
            let mut data = self.state.data.write().unwrap();
            match baseline.as_ref().and_then(|b| b.get(field)).cloned() {
                Some(measured) => {
                    data.insert(field.to_string(), measured);
                }
                None => {
                    data.shift_remove(field);
                }
            }
        }
        self.state.bump_generation();
    }

    /// Release every field back to tracking.
    pub fn revert_all(&self) {
        {
            let baseline = self.state.baseline.read().unwrap();
            let mut data = self.state.data.write().unwrap();
            *data = baseline.clone().unwrap_or_default();
        }
        self.state.bump_generation();
    }
}
