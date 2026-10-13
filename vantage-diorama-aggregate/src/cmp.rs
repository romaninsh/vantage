//! Column access and scalar views shared by the built-in aggregations.
//! Ordering, conditions and search go through vista's evaluator
//! ([`vantage_vista::eval`]).

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::eval::path_get;

/// Resolve a column against a record, walking dotted paths (`obj.field`) into
/// nested CBOR maps. Tries the literal key first, so a column whose name
/// genuinely contains a dot still resolves.
pub fn column<'a>(record: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    record.get(path).or_else(|| path_get(record, path))
}

/// Render a CBOR scalar as text, or `None` for shapes that aren't scalars
/// (maps, arrays, bytes, null). Tags are transparent — a tagged datetime or
/// record id renders as whatever it wraps.
///
/// Used to compare a configured value against a column whose CBOR type is
/// unknown to the configuration; deliberately `Option` so a non-scalar never
/// silently compares equal to an empty string.
pub fn scalar_text(value: &CborValue) -> Option<String> {
    match value {
        CborValue::Text(s) => Some(s.clone()),
        CborValue::Integer(i) => Some(i128::from(*i).to_string()),
        CborValue::Float(f) => Some(f.to_string()),
        CborValue::Bool(b) => Some(b.to_string()),
        CborValue::Tag(_, inner) => scalar_text(inner),
        _ => None,
    }
}

/// Numeric view of a CBOR scalar, for the numeric aggregations. `None` for
/// anything that is not a number.
pub fn as_f64(value: &CborValue) -> Option<f64> {
    match value {
        CborValue::Integer(i) => Some(i128::from(*i) as f64),
        CborValue::Float(f) => Some(*f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> CborValue {
        CborValue::Text(s.to_string())
    }

    #[test]
    fn dotted_paths_descend_into_nested_maps() {
        let mut record = Record::new();
        record.insert(
            "customer".to_string(),
            CborValue::Map(vec![(text("country"), text("LV"))]),
        );
        assert_eq!(column(&record, "customer.country"), Some(&text("LV")));
        assert_eq!(column(&record, "customer.missing"), None);
    }

    #[test]
    fn a_literal_dotted_key_wins_over_path_descent() {
        let mut record = Record::new();
        record.insert("a.b".to_string(), CborValue::Integer(7.into()));
        assert_eq!(column(&record, "a.b"), Some(&CborValue::Integer(7.into())));
    }
}
