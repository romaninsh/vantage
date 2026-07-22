//! Pure servo-loop arithmetic — no locks, no async, exhaustively testable.
//!
//! The vocabulary is a servomechanism's: `data` holds the commanded
//! setpoints, `baseline` the measured upstream state, and the **error
//! signal** is their per-field difference. A field with zero error is in
//! *continuous tracking* (it follows the measurement); a field with a
//! commanded setpoint is *locked* (it holds until actuated or released).

use ciborium::Value as CborValue;
use vantage_types::Record;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.to_string())
    }

    fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
        let mut r = Record::new();
        for (k, v) in pairs {
            r.insert((*k).to_string(), text(v));
        }
        r
    }

    #[test]
    fn zero_error_when_data_matches_baseline() {
        let baseline = rec(&[("a", "1"), ("b", "2")]);
        let data = baseline.clone();
        assert!(error_of(Some(&baseline), &data).is_empty());
    }

    #[test]
    fn error_carries_only_differing_fields() {
        let baseline = rec(&[("a", "1"), ("b", "2")]);
        let data = rec(&[("a", "1"), ("b", "CHANGED")]);
        let error = error_of(Some(&baseline), &data);
        assert_eq!(error.len(), 1);
        assert_eq!(error.get("b"), Some(&text("CHANGED")));
    }

    #[test]
    fn without_baseline_every_field_is_error() {
        let data = rec(&[("a", "1"), ("b", "2")]);
        assert_eq!(error_of(None, &data).len(), 2);
    }

    #[test]
    fn tracking_fields_adopt_the_measurement() {
        let mut baseline = Some(rec(&[("a", "1"), ("b", "2")]));
        let mut data = rec(&[("a", "1"), ("b", "2")]);
        absorb(
            &mut baseline,
            &mut data,
            Some(rec(&[("a", "9"), ("b", "2")])),
        );
        assert_eq!(data.get("a"), Some(&text("9")));
        assert!(error_of(baseline.as_ref(), &data).is_empty(), "still clean");
    }

    #[test]
    fn locked_fields_hold_their_setpoint() {
        let mut baseline = Some(rec(&[("a", "1"), ("b", "2")]));
        let mut data = rec(&[("a", "LOCKED"), ("b", "2")]);
        absorb(
            &mut baseline,
            &mut data,
            Some(rec(&[("a", "9"), ("b", "7")])),
        );
        assert_eq!(data.get("a"), Some(&text("LOCKED")), "setpoint held");
        assert_eq!(data.get("b"), Some(&text("7")), "tracking field adopted");
        let error = error_of(baseline.as_ref(), &data);
        assert_eq!(error.len(), 1, "only the locked field is error");
    }

    #[test]
    fn convergence_releases_the_lock() {
        let mut baseline = Some(rec(&[("a", "1")]));
        let mut data = rec(&[("a", "TARGET")]);
        absorb(&mut baseline, &mut data, Some(rec(&[("a", "TARGET")])));
        assert!(error_of(baseline.as_ref(), &data).is_empty());
    }

    #[test]
    fn new_upstream_fields_arrive_clean() {
        let mut baseline = Some(rec(&[("a", "1")]));
        let mut data = rec(&[("a", "1")]);
        absorb(
            &mut baseline,
            &mut data,
            Some(rec(&[("a", "1"), ("new", "x")])),
        );
        assert_eq!(data.get("new"), Some(&text("x")));
        assert!(error_of(baseline.as_ref(), &data).is_empty());
    }

    #[test]
    fn tracking_fields_follow_upstream_removal_locked_additions_survive() {
        let mut baseline = Some(rec(&[("gone", "1"), ("kept", "2")]));
        let mut data = rec(&[("gone", "1"), ("kept", "2"), ("added", "MINE")]);
        absorb(&mut baseline, &mut data, Some(rec(&[("kept", "2")])));
        assert!(
            data.get("gone").is_none(),
            "tracking field dropped with upstream"
        );
        assert_eq!(
            data.get("added"),
            Some(&text("MINE")),
            "commanded addition held"
        );
    }

    #[test]
    fn vanished_record_clears_the_baseline_and_keeps_setpoints() {
        let mut baseline = Some(rec(&[("a", "1")]));
        let mut data = rec(&[("a", "EDIT")]);
        absorb(&mut baseline, &mut data, None);
        assert!(baseline.is_none());
        assert_eq!(data.get("a"), Some(&text("EDIT")));
        assert_eq!(error_of(baseline.as_ref(), &data).len(), 1);
    }

    #[test]
    fn absorb_into_empty_servo_adopts_everything() {
        let mut baseline = None;
        let mut data = Record::new();
        absorb(
            &mut baseline,
            &mut data,
            Some(rec(&[("a", "1"), ("b", "2")])),
        );
        assert_eq!(data.len(), 2);
        assert!(error_of(baseline.as_ref(), &data).is_empty());
    }
}
