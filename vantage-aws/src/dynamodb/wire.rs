//! AttributeValue ↔ JSON wire codec.
//!
//! DynamoDB's JSON protocol tags every value with its type:
//! `{"S": "..."}`, `{"N": "42"}`, `{"BOOL": true}`. This module folds
//! that wire shape into our `AttributeValue` enum and back.
//!
//! Binary (`B`/`BS`) is intentionally not yet wired — base64 decode
//! would pull a new dependency and v0 doesn't need it.

use indexmap::IndexMap;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use vantage_core::{Result, error};

use super::types::AttributeValue;

/// `AttributeValue` → wire JSON object (`{"S": "x"}` etc).
pub(crate) fn attr_to_json(av: &AttributeValue) -> Result<JsonValue> {
    Ok(match av {
        AttributeValue::S(s) => json!({ "S": s }),
        AttributeValue::N(n) => json!({ "N": n }),
        AttributeValue::Bool(b) => json!({ "BOOL": b }),
        AttributeValue::Null => json!({ "NULL": true }),
        AttributeValue::L(arr) => {
            let items: Result<Vec<_>> = arr.iter().map(attr_to_json).collect();
            json!({ "L": items? })
        }
        AttributeValue::M(map) => {
            let mut obj = JsonMap::new();
            for (k, v) in map {
                obj.insert(k.clone(), attr_to_json(v)?);
            }
            json!({ "M": obj })
        }
        AttributeValue::SS(s) => json!({ "SS": s }),
        AttributeValue::NS(s) => json!({ "NS": s }),
        AttributeValue::B(_) | AttributeValue::BS(_) => {
            return Err(error!(
                "DynamoDB Binary AttributeValue not yet supported on the wire"
            ));
        }
    })
}

/// Wire JSON object → `AttributeValue`.
pub(crate) fn json_to_attr(value: &JsonValue) -> Result<AttributeValue> {
    let obj = value
        .as_object()
        .ok_or_else(|| error!("AttributeValue must be a JSON object"))?;
    let (tag, val) = obj
        .iter()
        .next()
        .ok_or_else(|| error!("AttributeValue object is empty"))?;

    Ok(match tag.as_str() {
        "S" => AttributeValue::S(
            val.as_str()
                .ok_or_else(|| error!("S value must be string"))?
                .to_string(),
        ),
        "N" => AttributeValue::N(
            val.as_str()
                .ok_or_else(|| error!("N value must be string"))?
                .to_string(),
        ),
        "BOOL" => AttributeValue::Bool(
            val.as_bool()
                .ok_or_else(|| error!("BOOL value must be bool"))?,
        ),
        "NULL" => AttributeValue::Null,
        "L" => {
            let arr = val
                .as_array()
                .ok_or_else(|| error!("L value must be array"))?;
            let items: Result<Vec<_>> = arr.iter().map(json_to_attr).collect();
            AttributeValue::L(items?)
        }
        "M" => {
            let map_obj = val
                .as_object()
                .ok_or_else(|| error!("M value must be object"))?;
            let mut map = IndexMap::with_capacity(map_obj.len());
            for (k, v) in map_obj {
                map.insert(k.clone(), json_to_attr(v)?);
            }
            AttributeValue::M(map)
        }
        "SS" => {
            let arr = val
                .as_array()
                .ok_or_else(|| error!("SS value must be array"))?;
            let items: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            AttributeValue::SS(items)
        }
        "NS" => {
            let arr = val
                .as_array()
                .ok_or_else(|| error!("NS value must be array"))?;
            let items: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            AttributeValue::NS(items)
        }
        "B" | "BS" => {
            return Err(error!(
                "DynamoDB Binary AttributeValue not yet supported on the wire",
                tag = tag.as_str()
            ));
        }
        other => {
            return Err(error!(
                "Unknown AttributeValue tag",
                tag = other.to_string()
            ));
        }
    })
}

/// Parse a wire item (object of `field → AttributeValue`) into ordered
/// `(field, AttributeValue)` pairs.
pub(crate) fn json_to_item_map(item: &JsonValue) -> Result<Vec<(String, AttributeValue)>> {
    let obj = item
        .as_object()
        .ok_or_else(|| error!("DynamoDB item must be a JSON object"))?;
    let mut out = Vec::with_capacity(obj.len());
    for (k, v) in obj {
        out.push((k.clone(), json_to_attr(v)?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s_round_trip() {
        let av = AttributeValue::S("hello".into());
        let j = attr_to_json(&av).unwrap();
        assert_eq!(j, json!({ "S": "hello" }));
        assert_eq!(json_to_attr(&j).unwrap(), av);
    }

    #[test]
    fn n_round_trip() {
        let av = AttributeValue::N("42".into());
        let j = attr_to_json(&av).unwrap();
        assert_eq!(j, json!({ "N": "42" }));
        assert_eq!(json_to_attr(&j).unwrap(), av);
    }

    #[test]
    fn bool_round_trip() {
        let av = AttributeValue::Bool(true);
        let j = attr_to_json(&av).unwrap();
        assert_eq!(j, json!({ "BOOL": true }));
        assert_eq!(json_to_attr(&j).unwrap(), av);
    }

    #[test]
    fn null_round_trip() {
        let av = AttributeValue::Null;
        let j = attr_to_json(&av).unwrap();
        assert_eq!(j, json!({ "NULL": true }));
        assert_eq!(json_to_attr(&j).unwrap(), av);
    }

    #[test]
    fn binary_errors() {
        let av = AttributeValue::B(vec![1, 2, 3]);
        assert!(attr_to_json(&av).is_err());
    }
}
