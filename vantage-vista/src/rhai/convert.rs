//! Value conversions between the CBOR carrier, Rhai `Dynamic`, and (for the
//! fetch surface) `serde_json`.
//!
//! Shared by [`rhai_conventional`](crate::rhai_conventional) (which seeds the
//! parent `row` map and lowers condition scalars) and
//! [`rhai_fetch`](crate::rhai_fetch) (which materialises fetched records and
//! renders a script's final value as JSON). Kept in one place so the two
//! vocabularies can never drift in how they round-trip a value.

use ciborium::Value as CborValue;
use rhai::{Array, Dynamic, EvalAltResult, Map as RhaiMap};
use vantage_types::Record;

/// Convert a scalar Rhai value into the universal CBOR carrier. Non-scalar
/// types (arrays, maps) are rejected — only condition/id values pass through.
pub(crate) fn dynamic_to_cbor(d: Dynamic) -> Result<CborValue, Box<EvalAltResult>> {
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
    } else {
        Err(format!(
            "cannot convert rhai value of type '{}' into a condition value",
            d.type_name()
        )
        .into())
    }
}

/// CBOR → Rhai `Dynamic`. Tagged values render their presentation form
/// (record ids as `"table:id"`, datetimes/UUIDs/decimals as their text —
/// the same shapes the UI grid shows) instead of degrading to unit.
pub(crate) fn cbor_to_dynamic(v: &CborValue) -> Dynamic {
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
pub(crate) fn record_to_dynamic(rec: &Record<CborValue>) -> Dynamic {
    let mut map = RhaiMap::new();
    for (k, v) in rec.iter() {
        map.insert(k.as_str().into(), cbor_to_dynamic(v));
    }
    Dynamic::from_map(map)
}

/// A Rhai map back into a record (used to pass a parent row to `get_ref`).
pub(crate) fn map_to_record(map: RhaiMap) -> Result<Record<CborValue>, Box<EvalAltResult>> {
    let mut out: Vec<(String, CborValue)> = Vec::with_capacity(map.len());
    for (k, v) in map {
        out.push((k.to_string(), dynamic_to_cbor(v)?));
    }
    Ok(out.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_record_id_renders_as_table_colon_id() {
        // A SurrealDB Thing used to degrade to UNIT here, so the same row
        // rendered differently in a data script than in the UI grid.
        let thing = CborValue::Tag(
            8,
            Box::new(CborValue::Array(vec![
                CborValue::Text("user".into()),
                CborValue::Text("1".into()),
            ])),
        );
        assert_eq!(cbor_to_dynamic(&thing).into_string().unwrap(), "user:1");
    }

    #[test]
    fn tagged_datetime_renders_inner_text() {
        let dt = CborValue::Tag(0, Box::new(CborValue::Text("2026-01-01T00:00:00Z".into())));
        assert_eq!(
            cbor_to_dynamic(&dt).into_string().unwrap(),
            "2026-01-01T00:00:00Z"
        );
    }

    #[test]
    fn big_integer_becomes_decimal_string_not_wraparound() {
        let n = i128::from(i64::MIN) - 1;
        let big = CborValue::Integer(n.try_into().unwrap());
        assert_eq!(cbor_to_dynamic(&big).into_string().unwrap(), n.to_string());
    }
}

/// Rhai `Dynamic` → `serde_json::Value` for handing a script's result back to a
/// caller (e.g. over MCP). `rhai` is built without its `serde` feature, so the
/// conversion is explicit; unknown types fall back to their display string.
pub(crate) fn dynamic_to_json(d: &Dynamic) -> serde_json::Value {
    use serde_json::Value;
    if d.is_unit() {
        return Value::Null;
    }
    if d.is::<bool>() {
        return Value::Bool(d.as_bool().unwrap_or(false));
    }
    if d.is::<i64>() {
        return Value::from(d.as_int().unwrap_or(0));
    }
    if d.is::<f64>() {
        return d
            .as_float()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null);
    }
    if d.is::<String>() {
        return Value::String(d.clone().into_string().unwrap_or_default());
    }
    if d.is::<Array>() {
        let arr = d.clone().cast::<Array>();
        return Value::Array(arr.iter().map(dynamic_to_json).collect());
    }
    if d.is::<RhaiMap>() {
        let map = d.clone().cast::<RhaiMap>();
        let obj = map
            .iter()
            .map(|(k, v)| (k.to_string(), dynamic_to_json(v)))
            .collect();
        return Value::Object(obj);
    }
    Value::String(d.to_string())
}
