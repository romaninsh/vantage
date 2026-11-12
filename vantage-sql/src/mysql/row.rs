//! sqlx rows and bind values for MySQL. Binding follows the variant tag of
//! `AnyMysqlType`; reading infers the variant from the column type and keeps
//! the value as CBOR, so decimals stay tagged strings and booleans stay
//! distinct from integers.

use ciborium::Value as CborValue;
use sqlx::mysql::MySqlRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use super::types::{AnyMysqlType, MysqlTypeVariants};

pub(crate) type MysqlQuery<'q> = sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>;

/// A value the binder can't express as its variant's parameter comes from
/// user data, so it surfaces as an error: never a silent NULL, never a
/// panic. Decimals, dates and times bind as their text, which MySQL parses.
pub(crate) fn bind_mysql_value<'q>(
    query: MysqlQuery<'q>,
    value: &'q AnyMysqlType,
) -> Result<MysqlQuery<'q>> {
    let cbor = value.value();
    let Some(variant) = value.type_variant() else {
        return bind_by_cbor(query, cbor);
    };
    let refused = || refused(variant, cbor);
    Ok(match variant {
        MysqlTypeVariants::Null => query.bind(None::<String>),
        MysqlTypeVariants::Bool => match cbor {
            CborValue::Null => query.bind(None::<bool>),
            CborValue::Bool(b) => query.bind(*b),
            CborValue::Integer(i) => query.bind(i128::from(*i) != 0),
            _ => return Err(refused()),
        },
        MysqlTypeVariants::Int2 => match cbor {
            CborValue::Null => query.bind(None::<i16>),
            CborValue::Integer(i) => query.bind(i16::try_from(*i).map_err(|_| refused())?),
            _ => return Err(refused()),
        },
        MysqlTypeVariants::Int4 => match cbor {
            CborValue::Null => query.bind(None::<i32>),
            CborValue::Integer(i) => query.bind(i32::try_from(*i).map_err(|_| refused())?),
            _ => return Err(refused()),
        },
        MysqlTypeVariants::Int8 => match cbor {
            CborValue::Null => query.bind(None::<i64>),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).map_err(|_| refused())?),
            _ => return Err(refused()),
        },
        MysqlTypeVariants::Float4 => match cbor {
            CborValue::Null => query.bind(None::<f32>),
            CborValue::Float(f) => query.bind(*f as f32),
            CborValue::Integer(i) => query.bind(i128::from(*i) as f32),
            _ => return Err(refused()),
        },
        MysqlTypeVariants::Float8 => match cbor {
            CborValue::Null => query.bind(None::<f64>),
            CborValue::Float(f) => query.bind(*f),
            CborValue::Integer(i) => query.bind(i128::from(*i) as f64),
            _ => return Err(refused()),
        },
        MysqlTypeVariants::Text => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Text(s) => query.bind(s.as_str()),
            CborValue::Tag(_, inner) => match inner.as_ref() {
                CborValue::Text(s) => query.bind(s.as_str()),
                _ => return Err(refused()),
            },
            _ => return Err(refused()),
        },
        MysqlTypeVariants::Decimal => text_of(query, cbor, 10).ok_or_else(refused)?,
        MysqlTypeVariants::DateTime => text_of(query, cbor, 0).ok_or_else(refused)?,
        MysqlTypeVariants::Date => text_of(query, cbor, 100).ok_or_else(refused)?,
        MysqlTypeVariants::Time => text_of(query, cbor, 101).ok_or_else(refused)?,
        MysqlTypeVariants::Blob => match cbor {
            CborValue::Null => query.bind(None::<Vec<u8>>),
            CborValue::Bytes(b) => query.bind(b.as_slice()),
            CborValue::Text(s) => query.bind(s.as_bytes()),
            _ => return Err(refused()),
        },
    })
}

/// NULL, or the text of `value`, plain or inside `tag`, bound as text;
/// `None` for any other shape.
fn text_of<'q>(query: MysqlQuery<'q>, value: &'q CborValue, tag: u64) -> Option<MysqlQuery<'q>> {
    match value {
        CborValue::Null => Some(query.bind(None::<String>)),
        CborValue::Text(s) => Some(query.bind(s.as_str())),
        CborValue::Tag(t, inner) if *t == tag => match inner.as_ref() {
            CborValue::Text(s) => Some(query.bind(s.as_str())),
            _ => None,
        },
        _ => None,
    }
}

fn refused(variant: MysqlTypeVariants, value: &CborValue) -> VantageError {
    error!(
        format!(
            "cannot bind {} as a MySQL {variant:?} parameter",
            AnyMysqlType::untyped(value.clone())
        ),
        variant = format!("{variant:?}")
    )
    .mark_invalid_value()
}

/// Bind a CBOR value without type variant — infers from the value itself.
/// JSON-shaped values (maps, arrays) bind as their JSON text.
fn bind_by_cbor<'q>(query: MysqlQuery<'q>, cbor: &'q CborValue) -> Result<MysqlQuery<'q>> {
    Ok(match cbor {
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
        // Decimal, datetime, date, time: their text, which MySQL parses.
        CborValue::Tag(0 | 10 | 100 | 101, inner) => match inner.as_ref() {
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => return Err(unbindable(cbor)),
        },
        CborValue::Map(_) | CborValue::Array(_) => {
            query.bind(crate::types::cbor_to_json(cbor.clone()).to_string())
        }
        _ => return Err(unbindable(cbor)),
    })
}

fn unbindable(value: &CborValue) -> VantageError {
    error!(format!(
        "cannot bind {} as a MySQL parameter",
        AnyMysqlType::untyped(value.clone())
    ))
    .mark_invalid_value()
}

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
                // Untyped — allows try_get to attempt parsing as DateTime, Decimal, etc.
                return (CborValue::Text(v), None);
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
                        Box::new(CborValue::Text(v.format("%H:%M:%S%.f").to_string())),
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
        return (CborValue::Text(v), None);
    }

    // Intentional: surface decode failures so missing type handlers are noticed early.
    eprintln!(
        "vantage: failed to decode MySQL column '{}' (type '{}') — returning NULL",
        row.columns()[ordinal].name(),
        type_name,
    );
    (CborValue::Null, None)
}
