//! Declared-type affinity: an integer column stores canonical integer text as
//! an integer; a text column and an id column not declared integer store an
//! integer as decimal text. A `date`, `time` or `timestamp` column stores the
//! carrier form of [`vantage_types::temporal`] from any form its readers
//! accept; a date column also takes an instant at exactly midnight UTC (what
//! `today()` and `add_months` return, and what SurrealDB keeps a date as). A
//! value that isn't one is refused. Anything else, including cells of an
//! undeclared or [untyped](vantage_vista::Column::untyped) column, is left as is.

use ciborium::Value as CborValue;
use vantage_core::{Result, VantageError, error};
use vantage_types::{Record, temporal};
use vantage_vista::{Column, VistaMetadata};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Affinity {
    Integer,
    Text,
    Date,
    Time,
    Instant,
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
            Some("time") => Affinity::Time,
            Some("timestamp" | "datetime") => Affinity::Instant,
            _ => Affinity::None,
        }
    }

    fn is_temporal(self) -> bool {
        matches!(self, Affinity::Date | Affinity::Time | Affinity::Instant)
    }

    /// `value` of `column` in the stored form for this affinity. Null stays
    /// null; a value a temporal column can't read as its kind is refused.
    pub(crate) fn conform(self, column: &str, value: &CborValue) -> Result<CborValue> {
        Ok(match (self, value) {
            (_, CborValue::Null) => CborValue::Null,
            (Affinity::Integer, CborValue::Text(text)) => match text.parse::<i64>() {
                Ok(n) if n.to_string() == *text => CborValue::Integer(n.into()),
                _ => value.clone(),
            },
            (Affinity::Text, CborValue::Integer(i)) => CborValue::Text(i128::from(*i).to_string()),
            (Affinity::Date, _) => temporal::as_date(value)
                .or_else(|| temporal::midnight_date(value))
                .map(|day| temporal::date(&day))
                .ok_or_else(|| not_a(column, "date", value))?,
            (Affinity::Time, _) => temporal::as_time(value)
                .map(|t| temporal::time(&t))
                .ok_or_else(|| not_a(column, "time", value))?,
            (Affinity::Instant, _) => temporal::as_instant(value)
                .map(|at| temporal::instant(&at))
                .ok_or_else(|| not_a(column, "timestamp", value))?,
            _ => value.clone(),
        })
    }

    /// A stored `value` of `column` as a read returns it: a temporal cell in
    /// its carrier form, whatever form a write past the vista left; others as
    /// stored.
    pub(crate) fn read(self, column: &str, value: &CborValue) -> Result<CborValue> {
        match self.is_temporal() {
            true => self.conform(column, value),
            false => Ok(value.clone()),
        }
    }
}

fn not_a(column: &str, kind: &str, value: &CborValue) -> VantageError {
    error!(
        format!("Value is not a {kind}"),
        column = column,
        value = value
    )
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
) -> Result<()> {
    for (name, value) in record.iter_mut() {
        let affinity = Affinity::of(metadata, id_column, name);
        if affinity != Affinity::None {
            *value = affinity.conform(name, value)?;
        }
    }
    Ok(())
}

/// Every cell of a stored `record` as a read returns it (see [`Affinity::read`]).
pub(crate) fn read_record(
    metadata: &VistaMetadata,
    id_column: &str,
    mut record: Record<CborValue>,
) -> Result<Record<CborValue>> {
    for (name, value) in record.iter_mut() {
        *value = Affinity::of(metadata, id_column, name).read(name, value)?;
    }
    Ok(record)
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
