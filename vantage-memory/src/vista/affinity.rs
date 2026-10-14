//! Declared-type affinity: an integer column stores canonical integer text as
//! an integer; a text column and the id column store an integer as decimal
//! text. Anything else, including cells of an undeclared or
//! [untyped](vantage_vista::Column::untyped) column, is left as is.

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{Column, VistaMetadata};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Affinity {
    Integer,
    Text,
    /// Undeclared, untyped, computed, or of a type with no conversion.
    None,
}

impl Affinity {
    /// The affinity of `name` in `metadata`, whose id column is `id_column`.
    pub(crate) fn of(metadata: &VistaMetadata, id_column: &str, name: &str) -> Self {
        if name == id_column {
            return Affinity::Text;
        }
        metadata.columns.get(name).map_or(Affinity::None, column)
    }

    /// `value` in the stored form for this affinity.
    pub(crate) fn conform(self, value: &CborValue) -> CborValue {
        match (self, value) {
            (Affinity::Integer, CborValue::Text(text)) => match text.parse::<i64>() {
                Ok(n) if n.to_string() == *text => CborValue::Integer(n.into()),
                _ => value.clone(),
            },
            (Affinity::Text, CborValue::Integer(i)) => CborValue::Text(i128::from(*i).to_string()),
            _ => value.clone(),
        }
    }
}

fn column(column: &Column) -> Affinity {
    if column.is_computed() {
        return Affinity::None;
    }
    let ty = column.original_type.as_deref().map(str::to_ascii_lowercase);
    match ty.as_deref() {
        Some("int" | "integer" | "i64" | "i32" | "u64" | "u32" | "bigint" | "smallint") => {
            Affinity::Integer
        }
        Some("string" | "text" | "str" | "varchar") => Affinity::Text,
        _ => Affinity::None,
    }
}

/// Every cell of `record` in its stored form.
pub(crate) fn conform_record(
    metadata: &VistaMetadata,
    id_column: &str,
    record: &mut Record<CborValue>,
) {
    for (name, value) in record.iter_mut() {
        let affinity = Affinity::of(metadata, id_column, name);
        if affinity != Affinity::None {
            *value = affinity.conform(value);
        }
    }
}
