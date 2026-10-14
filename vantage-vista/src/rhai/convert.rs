//! Value conversions between the CBOR carrier and Rhai `Dynamic`.
//!
//! Shared by the conventional-reference code (which seeds the parent
//! `row` map and lowers condition scalars) and the fetch code (which
//! materialises fetched records). Public so the other data-layer crates
//! (diorama's servo, the faker effects) round-trip values the same way — one
//! converter, so a record id renders as `"table:id"` in every script.
//!
//! JSON goes through `vantage_rhai::{to_json, from_json}`.

use ciborium::Value as CborValue;
use vantage_rhai::rhai::{Array, Blob, Dynamic, EvalAltResult, Map as RhaiMap};
use vantage_types::Record;

/// Convert a Rhai value into the universal CBOR carrier. Scalars, arrays and
/// maps pass through (an array is what an `in` condition takes); a value with
/// no CBOR story — a closure, a custom type — is an error naming the type.
///
/// An instant (a host's `now()`) travels as the standard CBOR datetime — tag
/// 0 over RFC 3339 text — which every driver with a datetime type reads as
/// one.
pub fn dynamic_to_cbor(d: Dynamic) -> Result<CborValue, Box<EvalAltResult>> {
    if let Some(dt) = d.clone().try_cast::<chrono::DateTime<chrono::Utc>>() {
        return Ok(CborValue::Tag(
            0,
            Box::new(CborValue::Text(dt.to_rfc3339())),
        ));
    }
    if d.is_unit() {
        Ok(CborValue::Null)
    } else if d.is::<bool>() {
        Ok(CborValue::Bool(d.cast::<bool>()))
    } else if d.is::<i64>() {
        Ok(CborValue::Integer(d.cast::<i64>().into()))
    } else if d.is::<f64>() {
        Ok(CborValue::Float(d.cast::<f64>()))
    } else if d.is::<String>() {
        Ok(CborValue::Text(d.cast::<String>()))
    } else if d.is::<char>() {
        Ok(CborValue::Text(d.cast::<char>().to_string()))
    } else if d.is::<Blob>() {
        Ok(CborValue::Bytes(d.cast::<Blob>()))
    } else if d.is::<Array>() {
        let items = d
            .cast::<Array>()
            .into_iter()
            .map(dynamic_to_cbor)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CborValue::Array(items))
    } else if d.is::<RhaiMap>() {
        let pairs = d
            .cast::<RhaiMap>()
            .into_iter()
            .map(|(k, v)| Ok((CborValue::Text(k.to_string()), dynamic_to_cbor(v)?)))
            .collect::<Result<Vec<_>, Box<EvalAltResult>>>()?;
        Ok(CborValue::Map(pairs))
    } else {
        Err(format!(
            "no CBOR representation for rhai value of type '{}'",
            d.type_name()
        )
        .into())
    }
}

/// CBOR → Rhai `Dynamic`. Tagged values render their presentation form
/// (record ids as `"table:id"`, datetimes/UUIDs/decimals as their text —
/// the same shapes the UI grid shows) instead of degrading to unit.
pub fn cbor_to_dynamic(v: &CborValue) -> Dynamic {
    match v {
        CborValue::Null => Dynamic::UNIT,
        CborValue::Bool(b) => Dynamic::from_bool(*b),
        CborValue::Integer(i) => {
            let n: i128 = (*i).into();
            match i64::try_from(n) {
                Ok(v) => Dynamic::from_int(v),
                // Beyond i64 (rhai's only int): decimal string, not a
                // silent wrap-around.
                Err(_) => Dynamic::from(n.to_string()),
            }
        }
        CborValue::Float(f) => Dynamic::from_float(*f),
        CborValue::Text(s) => Dynamic::from(s.clone()),
        CborValue::Bytes(b) => Dynamic::from_blob(b.clone()),
        CborValue::Array(a) => {
            let arr: Array = a.iter().map(cbor_to_dynamic).collect();
            Dynamic::from_array(arr)
        }
        CborValue::Map(m) => {
            let mut map = RhaiMap::new();
            for (k, val) in m {
                if let CborValue::Text(key) = k {
                    map.insert(key.as_str().into(), cbor_to_dynamic(val));
                }
            }
            Dynamic::from_map(map)
        }
        CborValue::Tag(..) => {
            // Normalise the tagged value to plain CBOR via its JSON
            // presentation, then convert that.
            let plain = vantage_types::json_to_cbor(vantage_types::cbor_to_json(
                &vantage_types::PresentationDialect,
                v.clone(),
            ));
            cbor_to_dynamic(&plain)
        }
        _ => Dynamic::UNIT,
    }
}

/// A whole record as a Rhai map (used to seed `row` and to materialise rows).
pub fn record_to_dynamic(rec: &Record<CborValue>) -> Dynamic {
    Dynamic::from_map(record_to_map(rec))
}

/// A whole record as a Rhai map.
pub fn record_to_map(rec: &Record<CborValue>) -> RhaiMap {
    let mut map = RhaiMap::new();
    for (k, v) in rec.iter() {
        map.insert(k.as_str().into(), cbor_to_dynamic(v));
    }
    map
}

/// A Rhai map back into a record (used to pass a parent row to `get_ref`).
pub fn map_to_record(map: RhaiMap) -> Result<Record<CborValue>, Box<EvalAltResult>> {
    let mut out: Vec<(String, CborValue)> = Vec::with_capacity(map.len());
    for (k, v) in map {
        out.push((k.to_string(), dynamic_to_cbor(v)?));
    }
    Ok(out.into_iter().collect())
}
