//! Helpers for converting between sqlx rows/values and vantage types.
//!
//! **Writing** (bind): Takes `AnyMysqlType` with variant tags — the variant
//! tells us exactly how to bind each value to sqlx.
//!
//! **Reading** (row): Returns `Record<AnyMysqlType>` with variant inferred from
//! the MySQL column type. Values are stored as `ciborium::Value` (CBOR) for
//! lossless type preservation — decimals stay as tagged strings, datetimes
//! keep their type identity, booleans aren't confused with integers.

use ciborium::Value as CborValue;
use sqlx::mysql::MySqlRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_types::Record;

use super::types::{AnyMysqlType, MysqlTypeVariants};

/// Bind an AnyMysqlType to a sqlx query.
pub(crate) fn bind_mysql_value<'q>(
    query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    value: &'q AnyMysqlType,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    let cbor = value.value();
    match value.type_variant() {
        Some(MysqlTypeVariants::Null) => query.bind(None::<String>),
        None => bind_by_cbor(query, cbor),
        Some(MysqlTypeVariants::Bool) => match cbor {
            CborValue::Null => query.bind(None::<bool>),
            CborValue::Bool(b) => query.bind(*b),
            CborValue::Integer(i) => match i64::try_from(*i) {
                Ok(n) => query.bind(n != 0),
                Err(_) => query.bind(None::<bool>),
            },
            _ => query.bind(None::<bool>),
        },
        Some(MysqlTypeVariants::Int2) => match cbor {
            CborValue::Null => query.bind(None::<i16>),
            CborValue::Integer(i) => {
                query.bind(i64::try_from(*i).ok().and_then(|n| i16::try_from(n).ok()))
            }
            _ => query.bind(None::<i16>),
        },
        Some(MysqlTypeVariants::Int4) => match cbor {
            CborValue::Null => query.bind(None::<i32>),
            CborValue::Integer(i) => {
                query.bind(i64::try_from(*i).ok().and_then(|n| i32::try_from(n).ok()))
            }
            _ => query.bind(None::<i32>),
        },
        Some(MysqlTypeVariants::Int8) => match cbor {
            CborValue::Null => query.bind(None::<i64>),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok()),
            _ => query.bind(None::<i64>),
        },
        Some(MysqlTypeVariants::Float4) => match cbor {
            CborValue::Null => query.bind(None::<f32>),
            CborValue::Float(f) => query.bind(*f as f32),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok().map(|n| n as f32)),
            _ => query.bind(None::<f32>),
        },
        Some(MysqlTypeVariants::Float8) => match cbor {
            CborValue::Null => query.bind(None::<f64>),
            CborValue::Float(f) => query.bind(*f),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok().map(|n| n as f64)),
            _ => query.bind(None::<f64>),
        },
        Some(MysqlTypeVariants::Text) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
        Some(MysqlTypeVariants::Decimal) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Tag(10, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    query.bind(s.as_str())
                } else {
                    query.bind(None::<String>)
                }
            }
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
        Some(MysqlTypeVariants::DateTime) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Tag(0, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    query.bind(s.as_str())
                } else {
                    query.bind(None::<String>)
                }
            }
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
        Some(MysqlTypeVariants::Date) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Tag(100, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    query.bind(s.as_str())
                } else {
                    query.bind(None::<String>)
                }
            }
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
        Some(MysqlTypeVariants::Time) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Tag(101, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    query.bind(s.as_str())
                } else {
                    query.bind(None::<String>)
                }
            }
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
        Some(MysqlTypeVariants::Blob) => match cbor {
            CborValue::Null => query.bind(None::<Vec<u8>>),
            CborValue::Bytes(b) => query.bind(b.as_slice()),
            CborValue::Text(s) => query.bind(s.as_bytes()),
            _ => query.bind(None::<Vec<u8>>),
        },
    }
}

/// Bind a CBOR value without type variant — infers from the value itself.
fn bind_by_cbor<'q>(
    query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    cbor: &'q CborValue,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    match cbor {
        CborValue::Null => query.bind(None::<String>),
        CborValue::Bool(b) => query.bind(*b),
        CborValue::Integer(i) => {
            if let Ok(n) = i64::try_from(*i) {
                query.bind(n)
            } else {
                query.bind(i128::from(*i).to_string())
            }
        }
        CborValue::Float(f) => query.bind(*f),
        CborValue::Text(s) => query.bind(s.as_str()),
        CborValue::Bytes(b) => query.bind(b.as_slice()),
        CborValue::Tag(10, inner) => {
            // Decimal — bind as string, MySQL will coerce
            if let CborValue::Text(s) = inner.as_ref() {
                query.bind(s.as_str())
            } else {
                query.bind(None::<String>)
            }
        }
        CborValue::Tag(0 | 100 | 101, inner) => {
            // DateTime / Date / Time — bind as string
            if let CborValue::Text(s) = inner.as_ref() {
                query.bind(s.as_str())
            } else {
                query.bind(None::<String>)
            }
        }
        _ => query.bind(None::<String>),
    }
}

/// Convert a MySqlRow to Record<AnyMysqlType>.
///
/// Each value is stored as CBOR with the type variant inferred from the
/// MySQL column type, preserving full type fidelity.
pub(crate) fn row_to_record(row: &MySqlRow) -> Record<AnyMysqlType> {
    let mut record = Record::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let type_name = col.type_info().name();
        let (cbor, variant) = mysql_column_to_cbor(row, col.ordinal(), type_name);
        let value = match variant {
            Some(v) => AnyMysqlType::with_variant(cbor, v),
            None => AnyMysqlType::untyped(cbor),
        };
        record.insert(name, value);
    }
    record
}

/// Read a single column from a MySQL row as CborValue, returning both the
/// value and the detected type variant.
fn mysql_column_to_cbor(
    row: &MySqlRow,
    ordinal: usize,
    type_name: &str,
) -> (CborValue, Option<MysqlTypeVariants>) {
    use sqlx::ValueRef;

    if row
        .try_get_raw(ordinal)
        .map(|v| v.is_null())
        .unwrap_or(true)
    {
        return (CborValue::Null, None);
    }

    match type_name {
        "BOOLEAN" | "BOOL" => {
            // MySQL BOOLEAN is TINYINT(1). sqlx decodes as i8.
            if let Ok(v) = row.try_get::<i8, _>(ordinal) {
                return (CborValue::Bool(v != 0), Some(MysqlTypeVariants::Bool));
            }
            if let Ok(v) = row.try_get::<bool, _>(ordinal) {
                return (CborValue::Bool(v), Some(MysqlTypeVariants::Bool));
            }
        }
        "TINYINT" => {
            if let Ok(v) = row.try_get::<i8, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(MysqlTypeVariants::Int2),
                );
            }
        }
        "TINYINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u8, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(MysqlTypeVariants::Int2),
                );
            }
        }
        "SMALLINT" => {
            if let Ok(v) = row.try_get::<i16, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(MysqlTypeVariants::Int2),
                );
            }
        }
        "SMALLINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u16, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(MysqlTypeVariants::Int2),
                );
            }
        }
        "INT" | "INTEGER" | "MEDIUMINT" => {
            if let Ok(v) = row.try_get::<i32, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(MysqlTypeVariants::Int4),
                );
            }
        }
        "INT UNSIGNED" | "MEDIUMINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u32, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(MysqlTypeVariants::Int4),
                );
            }
        }
        "BIGINT" => {
            if let Ok(v) = row.try_get::<i64, _>(ordinal) {
                return (CborValue::Integer(v.into()), Some(MysqlTypeVariants::Int8));
            }
        }
        "BIGINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u64, _>(ordinal) {
                return (CborValue::Integer(v.into()), Some(MysqlTypeVariants::Int8));
            }
        }
        "FLOAT" => {
            if let Ok(v) = row.try_get::<f32, _>(ordinal) {
                return (CborValue::Float(v as f64), Some(MysqlTypeVariants::Float4));
            }
        }
        "DOUBLE" => {
            if let Ok(v) = row.try_get::<f64, _>(ordinal) {
                return (CborValue::Float(v), Some(MysqlTypeVariants::Float8));
            }
        }
        "DECIMAL" | "NUMERIC" | "NEWDECIMAL" => {
            // Lossless: store decimal as Tag(10, Text("..."))
            if let Ok(v) = row.try_get::<rust_decimal::Decimal, _>(ordinal) {
                return (
                    CborValue::Tag(10, Box::new(CborValue::Text(v.to_string()))),
                    Some(MysqlTypeVariants::Decimal),
                );
            }
            // Fallback: try as string
            if let Ok(v) = row.try_get::<String, _>(ordinal) {
                return (
                    CborValue::Tag(10, Box::new(CborValue::Text(v))),
                    Some(MysqlTypeVariants::Decimal),
                );
            }
        }
        "JSON" => {
            if let Ok(v) = row.try_get::<serde_json::Value, _>(ordinal) {
                // JSON columns: convert the serde_json::Value to CBOR
                let cbor = crate::types::json_to_cbor(v);
                return (cbor, Some(MysqlTypeVariants::Text));
            }
        }
        "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "VARCHAR" | "CHAR" | "ENUM" | "SET" => {
            if let Ok(v) = row.try_get::<String, _>(ordinal) {
                return (CborValue::Text(v), Some(MysqlTypeVariants::Text));
            }
        }
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BINARY" | "VARBINARY" => {
            if let Ok(v) = row.try_get::<Vec<u8>, _>(ordinal) {
                return (CborValue::Bytes(v), Some(MysqlTypeVariants::Blob));
            }
        }
        "TIME" => {
            if let Ok(v) = row.try_get::<chrono::NaiveTime, _>(ordinal) {
                return (
                    CborValue::Tag(
                        101,
                        Box::new(CborValue::Text(v.format("%H:%M:%S").to_string())),
                    ),
                    Some(MysqlTypeVariants::Time),
                );
            }
        }
        "DATE" => {
            if let Ok(v) = row.try_get::<chrono::NaiveDate, _>(ordinal) {
                return (
                    CborValue::Tag(
                        100,
                        Box::new(CborValue::Text(v.format("%Y-%m-%d").to_string())),
                    ),
                    Some(MysqlTypeVariants::Date),
                );
            }
        }
        "DATETIME" => {
            if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(ordinal) {
                return (
                    CborValue::Tag(
                        0,
                        Box::new(CborValue::Text(
                            v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
                        )),
                    ),
                    Some(MysqlTypeVariants::DateTime),
                );
            }
        }
        "TIMESTAMP" | "TIMESTAMP(6)" => {
            if let Ok(v) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(ordinal) {
                return (
                    CborValue::Tag(
                        0,
                        Box::new(CborValue::Text(
                            v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
                        )),
                    ),
                    Some(MysqlTypeVariants::DateTime),
                );
            }
        }
        "YEAR" => {
            if let Ok(v) = row.try_get::<u16, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(MysqlTypeVariants::Int4),
                );
            }
        }
        "BIT" => {
            if let Ok(v) = row.try_get::<bool, _>(ordinal) {
                return (CborValue::Bool(v), Some(MysqlTypeVariants::Bool));
            }
            if let Ok(v) = row.try_get::<u64, _>(ordinal) {
                return (CborValue::Integer(v.into()), Some(MysqlTypeVariants::Int8));
            }
            if let Ok(bytes) = row.try_get::<Vec<u8>, _>(ordinal) {
                let v = bytes
                    .into_iter()
                    .fold(0u64, |acc, byte| (acc << 8) | u64::from(byte));
                return (CborValue::Integer(v.into()), Some(MysqlTypeVariants::Int8));
            }
        }
        _ => {}
    }

    // Fallback: try common types in order
    if let Ok(v) = row.try_get::<i64, _>(ordinal) {
        return (CborValue::Integer(v.into()), Some(MysqlTypeVariants::Int8));
    }
    if let Ok(v) = row.try_get::<i32, _>(ordinal) {
        return (
            CborValue::Integer((v as i64).into()),
            Some(MysqlTypeVariants::Int4),
        );
    }
    if let Ok(v) = row.try_get::<f64, _>(ordinal) {
        return (CborValue::Float(v), Some(MysqlTypeVariants::Float8));
    }
    if let Ok(v) = row.try_get::<String, _>(ordinal) {
        return (CborValue::Text(v), Some(MysqlTypeVariants::Text));
    }

    // Intentional: surface decode failures so missing type handlers are noticed early.
    eprintln!(
        "vantage: failed to decode MySQL column '{}' (type '{}') — returning NULL",
        row.columns()[ordinal].name(),
        type_name,
    );
    (CborValue::Null, None)
}
