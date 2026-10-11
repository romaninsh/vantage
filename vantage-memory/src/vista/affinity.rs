//! Declared-type affinity, SQLite-style: a cell is stored in the form its
//! declared column type names, so a seed file's `client_id: 1` and a join's
//! `"1"` meet. Only the integer/text pair converts, and only losslessly:
//!
//! - a column declared as an integer stores canonical integer text as an
//!   integer;
//! - a column declared as text, and the id column, store an integer as its
//!   decimal text.
//!
//! Every other value, and every cell of an undeclared column or of one
//! declared without a type, is left as is.

use std::collections::BTreeSet;

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{Column, VistaMetadata};

/// Columns declared without a type. Their metadata may still name one for
/// display (a spec defaults it to `string`), but nothing is converted.
pub(crate) type Untyped = BTreeSet<String>;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Affinity {
    Integer,
    Text,
    /// Undeclared, untyped, computed, or of a type with no conversion.
    None,
}

impl Affinity {
    /// The affinity of `name` in `metadata`, whose id column is `id_column`.
    pub(crate) fn of(
        metadata: &VistaMetadata,
        untyped: &Untyped,
        id_column: &str,
        name: &str,
    ) -> Self {
        if name == id_column {
            return Affinity::Text;
        }
        if untyped.contains(name) {
            return Affinity::None;
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
    match column.original_type.to_ascii_lowercase().as_str() {
        "int" | "integer" | "i64" | "i32" | "u64" | "u32" | "bigint" | "smallint" => {
            Affinity::Integer
        }
        "string" | "text" | "str" | "varchar" => Affinity::Text,
        _ => Affinity::None,
    }
}

/// Every cell of `record` in its stored form.
pub(crate) fn conform_record(
    metadata: &VistaMetadata,
    untyped: &Untyped,
    id_column: &str,
    record: &mut Record<CborValue>,
) {
    for (name, value) in record.iter_mut() {
        let affinity = Affinity::of(metadata, untyped, id_column, name);
        if affinity != Affinity::None {
            *value = affinity.conform(value);
        }
    }
}
