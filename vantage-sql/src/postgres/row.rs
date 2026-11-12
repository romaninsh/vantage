//! sqlx rows and bind values for PostgreSQL. Binding follows the variant tag of
//! `AnyPostgresType`; reading infers the variant from the column type and keeps
//! the value as CBOR, so decimals stay tagged strings and booleans stay
//! distinct from integers.

use ciborium::Value as CborValue;
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_core::{Result, VantageError, error};
use vantage_types::Record;

use super::types::{AnyPostgresType, PostgresTypeVariants};

pub(crate) type PgQuery<'q> = sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>;

/// A value the binder can't express as its variant's parameter comes from
/// user data, so it surfaces as an error: never a silent NULL, never a
/// panic. NULL binds as a NULL of the variant's type.
pub(crate) fn bind_postgres_value<'q>(
    query: PgQuery<'q>,
    value: &'q AnyPostgresType,
) -> Result<PgQuery<'q>> {
    let cbor = value.value();
    let Some(variant) = value.type_variant() else {
        return bind_by_cbor(query, cbor);
    };
    let refused = || refused(variant, cbor);
    Ok(match variant {
        PostgresTypeVariants::Null => query.bind(None::<String>),
        PostgresTypeVariants::Bool => match cbor {
            CborValue::Null => query.bind(None::<bool>),
            CborValue::Bool(b) => query.bind(*b),
            CborValue::Integer(i) => query.bind(i128::from(*i) != 0),
            _ => return Err(refused()),
        },
        PostgresTypeVariants::Int2 => match cbor {
            CborValue::Null => query.bind(None::<i16>),
            CborValue::Integer(i) => query.bind(i16::try_from(*i).map_err(|_| refused())?),
            _ => return Err(refused()),
        },
        PostgresTypeVariants::Int4 => match cbor {
            CborValue::Null => query.bind(None::<i32>),
            CborValue::Integer(i) => query.bind(i32::try_from(*i).map_err(|_| refused())?),
            CborValue::Array(items) => query.bind(int4_array(items).ok_or_else(refused)?),
            _ => return Err(refused()),
        },
        PostgresTypeVariants::Int8 => match cbor {
            CborValue::Null => query.bind(None::<i64>),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).map_err(|_| refused())?),
            _ => return Err(refused()),
        },
        PostgresTypeVariants::Float4 => match cbor {
            CborValue::Null => query.bind(None::<f32>),
            CborValue::Float(f) => query.bind(*f as f32),
            CborValue::Integer(i) => query.bind(i128::from(*i) as f32),
            _ => return Err(refused()),
        },
        PostgresTypeVariants::Float8 => match cbor {
            CborValue::Null => query.bind(None::<f64>),
            CborValue::Float(f) => query.bind(*f),
            CborValue::Integer(i) => query.bind(i128::from(*i) as f64),
            _ => return Err(refused()),
        },
        PostgresTypeVariants::Text => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Text(s) => query.bind(s.as_str()),
            CborValue::Tag(_, inner) => match inner.as_ref() {
                CborValue::Text(s) => query.bind(s.as_str()),
                _ => return Err(refused()),
            },
            CborValue::Array(items) => query.bind(text_array(items).ok_or_else(refused)?),
            _ => return Err(refused()),
        },
        PostgresTypeVariants::Decimal => match cbor {
            CborValue::Null => query.bind(None::<rust_decimal::Decimal>),
            _ => query.bind(
                tagged_text(cbor, 10)
                    .and_then(|s| s.parse::<rust_decimal::Decimal>().ok())
                    .ok_or_else(refused)?,
            ),
        },
        PostgresTypeVariants::DateTime => match cbor {
            CborValue::Null => query.bind(None::<chrono::NaiveDateTime>),
            _ => {
                let s = tagged_text(cbor, 0).ok_or_else(refused)?;
                // An offset makes it an instant (TIMESTAMPTZ); without one
                // it's a wall-clock time (TIMESTAMP).
                if let Some(dt) = parse_instant(s) {
                    query.bind(dt)
                } else {
                    query.bind(parse_naive_datetime(s).ok_or_else(refused)?)
                }
            }
        },
        PostgresTypeVariants::Date => match cbor {
            CborValue::Null => query.bind(None::<chrono::NaiveDate>),
            _ => query.bind(
                tagged_text(cbor, 100)
                    .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
                    .ok_or_else(refused)?,
            ),
        },
        PostgresTypeVariants::Time => match cbor {
            CborValue::Null => query.bind(None::<chrono::NaiveTime>),
            _ => query.bind(
                tagged_text(cbor, 101)
                    .and_then(|s| {
                        chrono::NaiveTime::parse_from_str(s, "%H:%M:%S")
                            .or_else(|_| chrono::NaiveTime::parse_from_str(s, "%H:%M:%S%.f"))
                            .ok()
                    })
                    .ok_or_else(refused)?,
            ),
        },
        PostgresTypeVariants::Uuid => match cbor {
            CborValue::Null => query.bind(None::<String>),
            _ => query.bind(tagged_text(cbor, 9).ok_or_else(refused)?),
        },
        PostgresTypeVariants::Blob => match cbor {
            CborValue::Null => query.bind(None::<Vec<u8>>),
            CborValue::Bytes(b) => query.bind(b.as_slice()),
            CborValue::Text(s) => query.bind(s.as_bytes()),
            _ => return Err(refused()),
        },
    })
}

/// The text of `value`, plain or inside `tag`.
fn tagged_text(value: &CborValue, tag: u64) -> Option<&str> {
    match value {
        CborValue::Text(s) => Some(s),
        CborValue::Tag(t, inner) if *t == tag => match inner.as_ref() {
            CborValue::Text(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

fn parse_instant(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%#z")
        .or_else(|_| chrono::DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%#z"))
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .ok()
        .or_else(|| s.parse::<chrono::DateTime<chrono::Utc>>().ok())
}

fn parse_naive_datetime(s: &str) -> Option<chrono::NaiveDateTime> {
    [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
    ]
    .iter()
    .find_map(|fmt| chrono::NaiveDateTime::parse_from_str(s, fmt).ok())
}

fn text_array(items: &[CborValue]) -> Option<Vec<String>> {
    items
        .iter()
        .map(|item| match item {
            CborValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn int4_array(items: &[CborValue]) -> Option<Vec<i32>> {
    items
        .iter()
        .map(|item| match item {
            CborValue::Integer(i) => i32::try_from(*i).ok(),
            _ => None,
        })
        .collect()
}

fn refused(variant: PostgresTypeVariants, value: &CborValue) -> VantageError {
    error!(
        format!(
            "cannot bind {} as a PostgreSQL {variant:?} parameter",
            AnyPostgresType::untyped(value.clone())
        ),
        variant = format!("{variant:?}")
    )
    .mark_invalid_value()
}

/// Bind a CBOR value without type variant — infers from the value itself.
/// JSON-shaped values (maps, arrays) bind as `jsonb`.
fn bind_by_cbor<'q>(query: PgQuery<'q>, cbor: &'q CborValue) -> Result<PgQuery<'q>> {
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
        // Decimal, uuid, datetime, date, time: their text, which
        // PostgreSQL coerces.
        CborValue::Tag(0 | 9 | 10 | 100 | 101, inner) => match inner.as_ref() {
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => return Err(unbindable(cbor)),
        },
        CborValue::Map(_) | CborValue::Array(_) => {
            query.bind(crate::types::cbor_to_json(cbor.clone()))
        }
        _ => return Err(unbindable(cbor)),
    })
}

fn unbindable(value: &CborValue) -> VantageError {
    error!(format!(
        "cannot bind {} as a PostgreSQL parameter",
        AnyPostgresType::untyped(value.clone())
    ))
    .mark_invalid_value()
}

/// Convert a `PgRow` to `Record<AnyPostgresType>`.
///
/// Each value is stored as CBOR with the type variant inferred from the
/// PostgreSQL column type, preserving full type fidelity.
pub(crate) fn row_to_record(row: &PgRow) -> Record<AnyPostgresType> {
    let mut record = Record::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let type_name = col.type_info().name();
        let (cbor, variant) = pg_column_to_cbor(row, col.ordinal(), type_name);
        let value = match variant {
            Some(v) => AnyPostgresType::with_variant(cbor, v),
            None => AnyPostgresType::untyped(cbor),
        };
        record.insert(name, value);
    }
    record
}

/// Read a single column from a PostgreSQL row as CborValue, returning both the
/// value and the detected type variant.
fn pg_column_to_cbor(
    row: &PgRow,
    ordinal: usize,
    type_name: &str,
) -> (CborValue, Option<PostgresTypeVariants>) {
    use sqlx::ValueRef;

    if row
        .try_get_raw(ordinal)
        .map(|v| v.is_null())
        .unwrap_or(true)
    {
        return (CborValue::Null, None);
    }

    match type_name {
        "BOOL" => {
            if let Ok(v) = row.try_get::<bool, _>(ordinal) {
                return (CborValue::Bool(v), Some(PostgresTypeVariants::Bool));
            }
        }
        "INT2" | "SMALLINT" | "SMALLSERIAL" => {
            if let Ok(v) = row.try_get::<i16, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(PostgresTypeVariants::Int2),
                );
            }
        }
        "INT4" | "INT" | "INTEGER" | "SERIAL" => {
            if let Ok(v) = row.try_get::<i32, _>(ordinal) {
                return (
                    CborValue::Integer((v as i64).into()),
                    Some(PostgresTypeVariants::Int4),
                );
            }
        }
        "INT8" | "BIGINT" | "BIGSERIAL" => {
            if let Ok(v) = row.try_get::<i64, _>(ordinal) {
                return (
                    CborValue::Integer(v.into()),
                    Some(PostgresTypeVariants::Int8),
                );
            }
        }
        "FLOAT4" | "REAL" => {
            if let Ok(v) = row.try_get::<f32, _>(ordinal) {
                return (
                    CborValue::Float(v as f64),
                    Some(PostgresTypeVariants::Float4),
                );
            }
        }
        "FLOAT8" | "DOUBLE PRECISION" => {
            if let Ok(v) = row.try_get::<f64, _>(ordinal) {
                return (CborValue::Float(v), Some(PostgresTypeVariants::Float8));
            }
        }
        "NUMERIC" | "DECIMAL" => {
            // Lossless: store decimal as Tag(10, Text("..."))
            if let Ok(v) = row.try_get::<rust_decimal::Decimal, _>(ordinal) {
                return (
                    CborValue::Tag(10, Box::new(CborValue::Text(v.to_string()))),
                    Some(PostgresTypeVariants::Decimal),
                );
            }
        }
        "_TEXT" | "TEXT[]" => {
            if let Ok(v) = row.try_get::<Vec<String>, _>(ordinal) {
                return (
                    CborValue::Array(v.into_iter().map(CborValue::Text).collect()),
                    Some(PostgresTypeVariants::Text),
                );
            }
        }
        "_INT4" | "INT4[]" | "INTEGER[]" => {
            if let Ok(v) = row.try_get::<Vec<i32>, _>(ordinal) {
                return (
                    CborValue::Array(
                        v.into_iter()
                            .map(|i| CborValue::Integer((i as i64).into()))
                            .collect(),
                    ),
                    Some(PostgresTypeVariants::Int4),
                );
            }
        }
        "UUID" => {
            if let Ok(v) = row.try_get::<uuid::Uuid, _>(ordinal) {
                return (
                    CborValue::Tag(9, Box::new(CborValue::Text(v.to_string()))),
                    Some(PostgresTypeVariants::Uuid),
                );
            }
        }
        "TIME" | "TIME WITHOUT TIME ZONE" => {
            if let Ok(v) = row.try_get::<chrono::NaiveTime, _>(ordinal) {
                return (
                    CborValue::Tag(
                        101,
                        Box::new(CborValue::Text(v.format("%H:%M:%S%.f").to_string())),
                    ),
                    Some(PostgresTypeVariants::Time),
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
                    Some(PostgresTypeVariants::Date),
                );
            }
        }
        "TIMESTAMPTZ" | "TIMESTAMP WITH TIME ZONE" => {
            if let Ok(v) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(ordinal) {
                return (
                    CborValue::Tag(
                        0,
                        Box::new(CborValue::Text(
                            v.format("%Y-%m-%d %H:%M:%S%.f+00").to_string(),
                        )),
                    ),
                    Some(PostgresTypeVariants::DateTime),
                );
            }
        }
        "TIMESTAMP" | "TIMESTAMP WITHOUT TIME ZONE" => {
            if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(ordinal) {
                return (
                    CborValue::Tag(
                        0,
                        Box::new(CborValue::Text(
                            v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
                        )),
                    ),
                    Some(PostgresTypeVariants::DateTime),
                );
            }
        }
        "JSONB" | "JSON" => {
            if let Ok(v) = row.try_get::<serde_json::Value, _>(ordinal) {
                let cbor = crate::types::json_to_cbor(v);
                return (cbor, None);
            }
        }
        "BYTEA" => {
            if let Ok(v) = row.try_get::<Vec<u8>, _>(ordinal) {
                return (CborValue::Bytes(v), Some(PostgresTypeVariants::Blob));
            }
        }
        _ => {}
    }

    if let Ok(v) = row.try_get::<bool, _>(ordinal) {
        return (CborValue::Bool(v), Some(PostgresTypeVariants::Bool));
    }
    if let Ok(v) = row.try_get::<i64, _>(ordinal) {
        return (
            CborValue::Integer(v.into()),
            Some(PostgresTypeVariants::Int8),
        );
    }
    if let Ok(v) = row.try_get::<i32, _>(ordinal) {
        return (
            CborValue::Integer((v as i64).into()),
            Some(PostgresTypeVariants::Int4),
        );
    }
    if let Ok(v) = row.try_get::<f64, _>(ordinal) {
        return (CborValue::Float(v), Some(PostgresTypeVariants::Float8));
    }
    if let Ok(v) = row.try_get::<String, _>(ordinal) {
        // Text fallback is untyped — allows try_get to attempt parsing as
        // DateTime, Decimal, etc. when the column is VARCHAR/TEXT.
        return (CborValue::Text(v), None);
    }

    // Intentional: surface decode failures so missing type handlers are noticed early.
    eprintln!(
        "vantage: failed to decode PostgreSQL column '{}' (type '{}') — returning NULL",
        row.columns()[ordinal].name(),
        type_name,
    );
    (CborValue::Null, None)
}
