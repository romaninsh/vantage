//! The column type vocabulary the SQL dialects share: each YAML alias and
//! the Rust type its column holds, plus the Rust-only column types a typed
//! `Table` may declare. One list feeds both the vista factory
//! (`column_for_type`) and each dialect's write typing (`coerce_column`), so
//! the two can't drift apart.
//!
//! YAML `bytes` is an integer byte count (rendered as "1.5 KB"), as on
//! SurrealDB; binary columns are a Rust-only `Vec<u8>` declaration.
//!
//! A written value is typed through the dialect's own type system: the
//! type's lenient decode (`coerce_cbor`), then `AnyX::new`, so the binder
//! sees the declared variant. No value (null or `""`) on a typed column is
//! a NULL of that type. `String` columns, and any type outside the list,
//! keep what they're given: a column without `type:` is a `String` column
//! too.

use ciborium::Value as CborValue;
use vantage_core::{VantageError, error};

/// Calls `$then! { $args yaml { … } rust { … } }` with the vocabulary.
macro_rules! sql_declared_types {
    ($then:ident ! $args:tt) => {
        $then! { $args
            yaml {
                ["int", "integer", "i64", "i32", "bytes"] => i64 as "int",
                ["float", "double", "f64", "f32"] => f64 as "float",
                ["bool", "boolean"] => bool as "bool",
                ["decimal", "numeric"] => rust_decimal::Decimal as "decimal",
                ["date"] => chrono::NaiveDate as "date",
                ["time"] => chrono::NaiveTime as "time",
                ["datetime"] => chrono::NaiveDateTime as "datetime",
                ["timestamp"] => chrono::DateTime<chrono::Utc> as "timestamp",
            }
            rust {
                Vec<u8> as "binary",
                i32 as "int", i16 as "int", i8 as "int",
                u32 as "int", u16 as "int", u8 as "int",
                f32 as "float",
            }
        }
    };
}
pub(crate) use sql_declared_types;

/// YAML types SQL has no column for. Refused at load rather than quietly
/// stored as text.
pub(crate) const UNSUPPORTED: &[&str] = &["duration"];

/// The vista factory's `column_for_type`: YAML type alias → typed `Column`,
/// erased to the dialect's value type.
#[cfg(feature = "vista")]
macro_rules! column_for_type_fn {
    (() yaml { $( [$($alias:literal),+] => $ty:ty as $name:literal ),* $(,)? }
        rust { $( $rty:ty as $rname:literal ),* $(,)? }) => {
        pub(crate) fn column_for_type<A: ColumnType>(name: &str, ty: &str) -> Result<Column<A>> {
            let col = match ty {
                "string" | "text" | "str" => Column::from_column(Column::<String>::new(name)),
                $( $($alias)|+ => Column::from_column(Column::<$ty>::new(name)), )*
                other if $crate::declared::UNSUPPORTED.contains(&other) => {
                    return Err(error!(
                        "YAML column type not supported by SQL drivers",
                        column = name,
                        ty = other.to_string()
                    ));
                }
                other => {
                    return Err(error!(
                        "Unknown YAML column type",
                        column = name,
                        ty = other.to_string()
                    ));
                }
            };
            Ok(col)
        }
    };
}
#[cfg(feature = "vista")]
pub(crate) use column_for_type_fn;

/// A dialect's `coerce_column(table, column, value)`, behind its
/// `TableSource::coerce_column_value`. `$db` is the datasource, `$trait`
/// its type-system trait and `$any` its value type.
macro_rules! sql_coerce_fn {
    (($db:ty, $trait:ident, $any:ident)
        yaml { $( [$($alias:literal),+] => $ty:ty as $name:literal ),* $(,)? }
        rust { $( $rty:ty as $rname:literal ),* $(,)? }) => {
        /// `value` as this dialect stores `column` of `table`: typed by the
        /// column's declared Rust type (see `crate::declared`).
        pub(crate) fn coerce_column<E: vantage_types::Entity<$any>>(
            table: &vantage_table::table::Table<$db, E>,
            column: &str,
            value: $any,
        ) -> vantage_core::Result<$any> {
            use vantage_table::traits::column_like::ColumnLike as _;
            let Some(declared) = table.columns().get(column) else {
                return Ok(value);
            };
            let type_name = $crate::declared::without_option(declared.get_type());
            $( if type_name == std::any::type_name::<$ty>() {
                return typed::<$ty>(column, $name, value);
            } )*
            $( if type_name == std::any::type_name::<$rty>() {
                return typed::<$rty>(column, $rname, value);
            } )*
            Ok(value)
        }

        fn typed<T: $trait>(column: &str, expected: &str, value: $any) -> vantage_core::Result<$any> {
            if $crate::declared::is_absent(value.value()) {
                return Ok($any::new(None::<T>));
            }
            match T::coerce_cbor(value.value().clone()) {
                Some(decoded) => Ok($any::new(decoded)),
                None => Err($crate::declared::invalid(column, expected, &value.to_string())),
            }
        }
    };
}
pub(crate) use sql_coerce_fn;

/// `T` for a declared `Option<T>`: an optional column types its values
/// as `T` does.
pub(crate) fn without_option(type_name: &str) -> &str {
    type_name
        .strip_prefix("core::option::Option<")
        .and_then(|t| t.strip_suffix('>'))
        .unwrap_or(type_name)
}

/// Null, or `""` as a cleared form field sends it.
pub(crate) fn is_absent(value: &CborValue) -> bool {
    match value {
        CborValue::Null => true,
        CborValue::Text(s) => s.is_empty(),
        _ => false,
    }
}

pub(crate) fn invalid(column: &str, expected: &str, value: &str) -> VantageError {
    error!(
        format!("`{column}`: {value} doesn't fit type `{expected}`"),
        column = column,
        expected = expected,
        value = value
    )
    .mark_invalid_value()
}
