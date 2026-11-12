//! The YAML column type vocabulary: each alias, the Rust type its column
//! holds, and how a written value is typed by that column. One list, so the
//! factory and the write path can't drift apart.
//!
//! A value is typed through the SurrealDB type system: the type's lenient
//! decode (`coerce_cbor`), then `AnySurrealType::new`. No value (null,
//! `NONE`, or `""`) on a typed column is `NONE`, the absent value an
//! `option<…>` field takes. `String` columns keep what they're given: a
//! column without `type:` is a `String` column too.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::Entity;

use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::{AnySurrealType, SurrealType, SurrealTypeMarker};

macro_rules! declared_types {
    ($( $(#[$meta:meta])* [$($alias:literal),+] => $ty:ty as $name:literal by $coerce:expr ),* $(,)?) => {
        /// YAML type alias → typed `Column` (then erased to
        /// `Column<AnySurrealType>`).
        pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<Column<AnySurrealType>> {
            match ty {
                $( $(#[$meta])* $($alias)|+ => Ok(Column::from_column(Column::<$ty>::new(name))), )*
                other => Err(error!(
                    "Unknown YAML column type",
                    column = name,
                    ty = other.to_string()
                )),
            }
        }

        /// The YAML name (`timestamp`) of a column's Rust type name.
        pub(crate) fn declared_name(type_name: &str) -> Option<&'static str> {
            let type_name = without_option(type_name);
            $( $(#[$meta])* {
                if type_name == std::any::type_name::<$ty>() {
                    return Some($name);
                }
            } )*
            None
        }

        /// `value` typed by a column of Rust type `type_name`; `None` for a
        /// type outside the vocabulary (`AnySurrealType`, say).
        fn coerce_declared(
            type_name: &str,
            column: &str,
            value: AnySurrealType,
        ) -> Option<Result<AnySurrealType>> {
            let type_name = without_option(type_name);
            $( $(#[$meta])* {
                if type_name == std::any::type_name::<$ty>() {
                    return Some($coerce(column, $name, value));
                }
            } )*
            None
        }
    };
}

declared_types! {
    ["int", "integer", "i64", "i32", "bytes"] => i64 as "int" by typed::<i64>,
    ["float", "double", "f64", "f32"] => f64 as "float" by typed::<f64>,
    ["bool", "boolean"] => bool as "bool" by typed::<bool>,
    ["string", "text", "str"] => String as "string" by keep,
    ["thing", "record", "record_id"] => Thing as "record" by record,
    ["timestamp", "datetime"] => DateTime<Utc> as "timestamp" by typed::<DateTime<Utc>>,
    ["date"] => NaiveDate as "date" by date,
    ["time"] => NaiveTime as "time" by typed::<NaiveTime>,
    ["duration"] => std::time::Duration as "duration" by typed::<std::time::Duration>,
    #[cfg(feature = "decimal")]
    ["decimal", "numeric"] => rust_decimal::Decimal as "decimal" by typed::<rust_decimal::Decimal>,
}

/// `value` as SurrealDB stores `column` of `table`; see the module docs. A
/// has-one key column holds record links to its target, whatever its
/// declared type.
pub(crate) fn coerce_column<E: Entity<AnySurrealType>>(
    table: &Table<SurrealDB, E>,
    column: &str,
    value: AnySurrealType,
) -> Result<AnySurrealType> {
    let typed = if let Some(target) = table.has_one_target(column) {
        link(&target, column, value)
    } else {
        let Some(declared) = table.columns().get(column) else {
            return Ok(value);
        };
        match coerce_declared(declared.get_type(), column, value.clone()) {
            Some(typed) => typed,
            None => return Ok(value),
        }
    };
    typed.map_err(|mut err| {
        err.context
            .insert("table".to_string(), format!("{:?}", table.table_name()));
        err
    })
}

/// `T` for a declared `Option<T>`: an optional column types its values as
/// `T` does.
fn without_option(type_name: &str) -> &str {
    type_name
        .strip_prefix("core::option::Option<")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(type_name)
}

fn is_absent(value: &CborValue) -> bool {
    match value {
        CborValue::Null | CborValue::Tag(6, _) => true,
        CborValue::Text(s) => s.is_empty(),
        _ => false,
    }
}

fn keep(_column: &str, _expected: &str, value: AnySurrealType) -> Result<AnySurrealType> {
    Ok(value)
}

/// A value already of `T`'s kind passes as it is, so a stored value written
/// back is unchanged; anything else goes through `T`'s lenient decode.
/// SurrealDB refuses tag 0 datetimes, so those are always re-encoded.
fn typed<T: SurrealType>(
    column: &str,
    expected: &str,
    value: AnySurrealType,
) -> Result<AnySurrealType> {
    let raw = value.value();
    if is_absent(raw) {
        return Ok(AnySurrealType::new(None::<T>));
    }
    if value.type_variant() == Some(T::Target::TYPE_ENUM)
        && !matches!(raw, CborValue::Tag(0, _))
        && T::from_cbor(raw.clone()).is_some()
    {
        return Ok(value);
    }
    match T::coerce_cbor(raw.clone()) {
        Some(decoded) => Ok(AnySurrealType::new(decoded)),
        None => Err(invalid(column, expected, &value)),
    }
}

/// A `date` column. A stored datetime (tag 12) passes as it is, midnight or
/// not: it is the store's own value (seed data, another writer), so a row
/// read and written back stays unchanged. Text and tagged dates are typed,
/// and refused when they carry a time of day.
fn date(column: &str, expected: &str, value: AnySurrealType) -> Result<AnySurrealType> {
    let raw = value.value();
    if matches!(raw, CborValue::Tag(12, _)) && DateTime::<Utc>::from_cbor(raw.clone()).is_some() {
        return Ok(value);
    }
    typed::<NaiveDate>(column, expected, value)
}

/// A `thing` column: a record id passes (an integer key too), `table:key`
/// text becomes one.
fn record(column: &str, expected: &str, value: AnySurrealType) -> Result<AnySurrealType> {
    match value.value() {
        raw if is_absent(raw) => Ok(AnySurrealType::new(None::<Thing>)),
        CborValue::Tag(8, _) => Ok(value),
        CborValue::Text(text) => match text.parse::<Thing>() {
            Ok(thing) => Ok(AnySurrealType::new(thing)),
            Err(_) => Err(invalid(column, expected, &value)),
        },
        _ => Err(invalid(column, expected, &value)),
    }
}

/// A has-one key column holding links to `target`: `target:key` text and a
/// bare key become `target`'s record id. Text naming another table is
/// refused: a `record<target>` field would refuse it anyway.
fn link(target: &str, column: &str, value: AnySurrealType) -> Result<AnySurrealType> {
    let expected = format!("record<{target}>");
    match value.value() {
        raw if is_absent(raw) => Ok(AnySurrealType::new(None::<Thing>)),
        CborValue::Tag(8, _) => Ok(value),
        CborValue::Text(text) => match text.split_once(':') {
            None => Ok(AnySurrealType::new(Thing::new(target, text.as_str()))),
            Some((table, key)) if table == target => {
                Ok(AnySurrealType::new(Thing::new(table, key)))
            }
            Some(_) => Err(invalid(column, &expected, &value)),
        },
        _ => Err(invalid(column, &expected, &value)),
    }
}

fn invalid(column: &str, expected: &str, value: &AnySurrealType) -> vantage_core::VantageError {
    error!(
        format!("`{column}`: {value} doesn't fit type `{expected}`"),
        column = column,
        expected = expected,
        value = value.to_string()
    )
    .mark_invalid_value()
}
