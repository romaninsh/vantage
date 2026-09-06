//! Value conversions between the CBOR carrier and Rhai `Dynamic`.
//!
//! Shared by [`conventional`](super::conventional) (which seeds the parent
//! `row` map and lowers condition scalars) and [`fetch`](super::fetch) (which
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
pub fn dynamic_to_cbor(d: Dynamic) -> Result<CborValue, Box<EvalAltResult>> {
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

    #[test]
    fn arrays_and_maps_round_trip() {
        let mut map = RhaiMap::new();
        map.insert(
            "ids".into(),
            Dynamic::from(vec![Dynamic::from(1_i64), Dynamic::from("x")]),
        );
        let cbor = dynamic_to_cbor(Dynamic::from(map)).unwrap();
        let CborValue::Map(pairs) = &cbor else {
            panic!("{cbor:?}")
        };
        assert_eq!(pairs[0].0, CborValue::Text("ids".into()));
        assert!(matches!(&pairs[0].1, CborValue::Array(a) if a.len() == 2));
        let back = cbor_to_dynamic(&cbor);
        assert!(back.is_map());
    }

    #[test]
    fn opaque_values_are_named_in_the_error() {
        #[derive(Clone)]
        struct Opaque;
        let err = dynamic_to_cbor(Dynamic::from(Opaque)).unwrap_err();
        assert!(err.to_string().contains("Opaque"), "{err}");
    }
}
