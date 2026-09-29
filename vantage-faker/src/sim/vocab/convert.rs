//! Value round-tripping between Rhai and CBOR (the scalar subset scripts
//! touch).

use ciborium::Value as CborValue;
use vantage_rhai::rhai::{Dynamic, Map as RhaiMap};
use vantage_types::Record;

pub(crate) fn dynamic_to_cbor(v: &Dynamic) -> CborValue {
    if v.is_unit() {
        CborValue::Null
    } else if let Ok(i) = v.as_int() {
        CborValue::Integer(i.into())
    } else if let Ok(f) = v.as_float() {
        CborValue::Float(f)
    } else if let Ok(b) = v.as_bool() {
        CborValue::Bool(b)
    } else {
        CborValue::Text(v.to_string())
    }
}

pub(crate) fn cbor_to_dynamic(v: &CborValue) -> Dynamic {
    match v {
        CborValue::Text(s) => Dynamic::from(s.clone()),
        CborValue::Integer(i) => Dynamic::from(i128::from(*i) as i64),
        CborValue::Float(f) => Dynamic::from(*f),
        CborValue::Bool(b) => Dynamic::from(*b),
        CborValue::Null => Dynamic::UNIT,
        other => Dynamic::from(format!("{other:?}")),
    }
}

pub(crate) fn record_to_map(rec: &Record<CborValue>) -> RhaiMap {
    rec.iter()
        .map(|(k, v)| (k.as_str().into(), cbor_to_dynamic(v)))
        .collect()
}

pub(crate) fn map_to_record(map: &RhaiMap) -> Record<CborValue> {
    let mut rec = Record::new();
    for (k, v) in map {
        rec.insert(k.to_string(), dynamic_to_cbor(v));
    }
    rec
}
