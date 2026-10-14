//! Rows the built-in aggregations' unit tests reduce.

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::VistaMetadata;

use crate::aggregate::aggregation::Rows;

pub(super) fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

/// `records` keyed by their position.
pub(super) fn rows(records: Vec<Record<CborValue>>) -> Rows {
    records
        .into_iter()
        .enumerate()
        .map(|(i, r)| (i.to_string(), r))
        .collect()
}

pub(super) fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

/// Two `opened` events (sizes 10 and 30) and one `failed` (size 20).
pub(super) fn events() -> Rows {
    rows(vec![
        record(&[
            ("Event", text("opened")),
            ("Size", CborValue::Integer(10.into())),
        ]),
        record(&[
            ("Event", text("failed")),
            ("Size", CborValue::Integer(20.into())),
        ]),
        record(&[
            ("Event", text("opened")),
            ("Size", CborValue::Integer(30.into())),
        ]),
    ])
}

/// The schema the tests' aggregations are handed: these rows have none.
pub(super) fn schema() -> VistaMetadata {
    VistaMetadata::new()
}
