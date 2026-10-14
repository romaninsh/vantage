//! Column access and scalar views shared by the built-in aggregations.
//! Ordering, conditions and search go through vista's evaluator
//! ([`vantage_vista::eval`]).

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::VistaMetadata;
use vantage_vista::eval::cell_get;

/// The cell `name` names in `row`, read as vista's evaluator reads it: a
/// dotted column `schema` declares is a flat key, any other dotted column a
/// path into nested maps.
pub(crate) fn column<'a>(
    row: &'a Record<CborValue>,
    name: &str,
    schema: &VistaMetadata,
) -> Option<&'a CborValue> {
    cell_get(row, name, schema)
}

/// Render a CBOR scalar as text, or `None` for shapes that aren't scalars
/// (maps, arrays, bytes, null). Tags are transparent — a tagged datetime or
/// record id renders as whatever it wraps.
///
/// Converts an operand for a text-typed column; deliberately `Option` so a
/// non-scalar is left as given rather than becoming an empty string.
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
    use vantage_vista::Column;

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
        let schema = VistaMetadata::new();
        assert_eq!(column(&record, "customer.country", &schema), Some(&text("LV")));
        assert_eq!(column(&record, "customer.missing", &schema), None);
    }

    #[test]
    fn a_declared_dotted_key_is_read_flat() {
        let mut record = Record::new();
        record.insert("a.b".to_string(), CborValue::Integer(7.into()));
        let declared = VistaMetadata::new().with_column(Column::untyped("a.b"));
        assert_eq!(
            column(&record, "a.b", &declared),
            Some(&CborValue::Integer(7.into()))
        );
        assert_eq!(column(&record, "a.b", &VistaMetadata::new()), None);
    }
}
