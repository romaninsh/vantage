//! Declared-type affinity: an integer column stores canonical integer text as
//! an integer; a text column and an id column not declared integer store an
//! integer as decimal text; a date column stores a date-only `Tag(100)` whether it is given
//! date-only text or a datetime at midnight UTC (what `today()` and
//! `add_months` return, and what SurrealDB keeps a date as). A datetime with a
//! time of day is left as given. Anything else, including cells of an
//! undeclared or [untyped](vantage_vista::Column::untyped) column, is left as is.

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{Column, VistaMetadata};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Affinity {
    Integer,
    Text,
    Date,
    /// Undeclared, untyped, computed, or of a type with no conversion.
    None,
}

impl Affinity {
    /// The affinity of `name` in `metadata`, whose id column is `id_column`.
    pub(crate) fn of(metadata: &VistaMetadata, id_column: &str, name: &str) -> Self {
        let declared = metadata.columns.get(name).map_or(Affinity::None, column);
        match name == id_column {
            true if declared == Affinity::Integer => Affinity::Integer,
            true => Affinity::Text,
            false => declared,
        }
    }

    /// The affinity of a column declared with type `ty`.
    pub(crate) fn of_type(ty: Option<&str>) -> Self {
        let ty = ty.map(str::to_ascii_lowercase);
        match ty.as_deref() {
            Some("int" | "integer" | "i64" | "i32" | "u64" | "u32" | "bigint" | "smallint") => {
                Affinity::Integer
            }
            Some("string" | "text" | "str" | "varchar") => Affinity::Text,
            Some("date") => Affinity::Date,
            _ => Affinity::None,
        }
    }

    /// `value` in the stored form for this affinity.
    pub(crate) fn conform(self, value: &CborValue) -> CborValue {
        match (self, value) {
            (Affinity::Integer, CborValue::Text(text)) => match text.parse::<i64>() {
                Ok(n) if n.to_string() == *text => CborValue::Integer(n.into()),
                _ => value.clone(),
            },
            (Affinity::Text, CborValue::Integer(i)) => CborValue::Text(i128::from(*i).to_string()),
            (Affinity::Date, CborValue::Text(text)) => {
                date_only(text).unwrap_or_else(|| value.clone())
            }
            (Affinity::Date, CborValue::Tag(0, inner)) => match inner.as_ref() {
                CborValue::Text(text) => date_only(text).unwrap_or_else(|| value.clone()),
                _ => value.clone(),
            },
            _ => value.clone(),
        }
    }
}

/// `Tag(100, "YYYY-MM-DD")` for date-only text or a datetime at midnight UTC.
fn date_only(text: &str) -> Option<CborValue> {
    let day = text.get(..10).filter(|day| is_date(day))?;
    let rest = text[10..].strip_prefix(['T', ' ']);
    let at_midnight = match rest {
        None => text.len() == 10,
        Some(rest) => rest.strip_prefix("00:00:00").is_some_and(|zone| {
            let zone = zone
                .strip_prefix('.')
                .map_or(zone, |f| f.trim_start_matches('0'));
            matches!(zone, "" | "Z" | "+00:00" | "+00" | "+0000")
        }),
    };
    at_midnight.then(|| CborValue::Tag(100, Box::new(CborValue::Text(day.to_string()))))
}

fn is_date(day: &str) -> bool {
    let b = day.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
}

fn column(column: &Column) -> Affinity {
    if column.is_computed() {
        return Affinity::None;
    }
    Affinity::of_type(column.original_type.as_deref())
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

/// Metadata read off a typed table names Rust types (`alloc::string::String`).
/// Each becomes its last path segment, and an `AnyMemoryType` column is
/// untyped, so the affinity rules read it as they read a spec's.
pub(crate) fn declared_types(mut metadata: VistaMetadata) -> VistaMetadata {
    for column in metadata.columns.values_mut() {
        column.original_type = match column.original_type.as_deref() {
            Some(ty) if ty.ends_with("AnyMemoryType") => None,
            Some(ty) => Some(ty.rsplit("::").next().unwrap_or(ty).to_string()),
            None => None,
        };
    }
    metadata
}
