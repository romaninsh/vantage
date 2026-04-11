//! Helpers for converting between sqlx rows/values and vantage types.
//!
//! **Writing** (bind): Takes `AnyPostgresType` with variant tags — the variant
//! tells us exactly how to bind each value to sqlx.
//!
//! **Reading** (row): Returns `Record<AnyPostgresType>` with variant inferred from
//! the PostgreSQL column type. Values are stored as `ciborium::Value` (CBOR) for
//! lossless type preservation — decimals stay as tagged strings, datetimes
//! keep their type identity, booleans aren't confused with integers.

use ciborium::Value as CborValue;
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_types::Record;

use super::types::{AnyPostgresType, PostgresTypeVariants};

/// Bind an AnyPostgresType to a sqlx query. Uses the variant tag to pick
/// the right sqlx bind type — no guessing from the CBOR value format.
pub(crate) fn bind_postgres_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: &'q AnyPostgresType,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    let cbor = value.value();
    match value.type_variant() {
        Some(PostgresTypeVariants::Null) => query.bind(None::<String>),
        None => bind_by_cbor(query, cbor),
        Some(PostgresTypeVariants::Bool) => match cbor {
            CborValue::Null => query.bind(None::<bool>),
            CborValue::Bool(b) => query.bind(*b),
            CborValue::Integer(i) => match i64::try_from(*i) {
                Ok(n) => query.bind(n != 0),
                Err(_) => query.bind(None::<bool>),
            },
            _ => query.bind(None::<bool>),
        },
        Some(PostgresTypeVariants::Int2) => match cbor {
            CborValue::Null => query.bind(None::<i16>),
            CborValue::Integer(i) => {
                query.bind(i64::try_from(*i).ok().and_then(|n| i16::try_from(n).ok()))
            }
            _ => query.bind(None::<i16>),
        },
        Some(PostgresTypeVariants::Int4) => match cbor {
            CborValue::Null => query.bind(None::<i32>),
            CborValue::Integer(i) => {
                query.bind(i64::try_from(*i).ok().and_then(|n| i32::try_from(n).ok()))
            }
            _ => query.bind(None::<i32>),
        },
        Some(PostgresTypeVariants::Int8) => match cbor {
            CborValue::Null => query.bind(None::<i64>),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok()),
            _ => query.bind(None::<i64>),
        },
        Some(PostgresTypeVariants::Float4) => match cbor {
            CborValue::Null => query.bind(None::<f32>),
            CborValue::Float(f) => query.bind(*f as f32),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok().map(|n| n as f32)),
            _ => query.bind(None::<f32>),
        },
        Some(PostgresTypeVariants::Float8) => match cbor {
            CborValue::Null => query.bind(None::<f64>),
            CborValue::Float(f) => query.bind(*f),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok().map(|n| n as f64)),
            _ => query.bind(None::<f64>),
        },
        Some(PostgresTypeVariants::Text) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
        Some(PostgresTypeVariants::Decimal) => match cbor {
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
        Some(PostgresTypeVariants::DateTime) => match cbor {
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
        Some(PostgresTypeVariants::Date) => match cbor {
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
        Some(PostgresTypeVariants::Time) => match cbor {
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
        Some(PostgresTypeVariants::Uuid) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Tag(9, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    query.bind(s.as_str())
                } else {
                    query.bind(None::<String>)
                }
            }
            CborValue::Text(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
        Some(PostgresTypeVariants::Blob) => match cbor {
            CborValue::Null => query.bind(None::<Vec<u8>>),
            CborValue::Bytes(b) => query.bind(b.as_slice()),
            CborValue::Text(s) => query.bind(s.as_bytes()),
            _ => query.bind(None::<Vec<u8>>),
        },
    }
}

/// Bind a CBOR value without type variant — infers from the value itself.
fn bind_by_cbor<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    cbor: &'q CborValue,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
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
            // Decimal — bind as string, PostgreSQL will coerce
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
        CborValue::Tag(9, inner) => {
            // UUID — bind as string
            if let CborValue::Text(s) = inner.as_ref() {
                query.bind(s.as_str())
            } else {
                query.bind(None::<String>)
            }
        }
        _ => query.bind(None::<String>),
    }
}

/// Convert a PgRow to Record<AnyPostgresType>.
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
        return (CborValue::Null, Some(PostgresTypeVariants::Null));
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
        // -- PostgreSQL array types --
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
                    CborValue::Tag(0, Box::new(CborValue::Text(v.to_rfc3339()))),
                    Some(PostgresTypeVariants::DateTime),
                );
            }
        }
        "TIMESTAMP" | "TIMESTAMP WITHOUT TIME ZONE" => {
            if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(ordinal) {
                return (
                    CborValue::Tag(
                        0,
                        Box::new(CborValue::Text(v.format("%Y-%m-%dT%H:%M:%S").to_string())),
                    ),
                    Some(PostgresTypeVariants::DateTime),
                );
            }
        }
        "JSONB" | "JSON" => {
            if let Ok(v) = row.try_get::<serde_json::Value, _>(ordinal) {
                let cbor = json_value_to_cbor(v);
                return (cbor, Some(PostgresTypeVariants::Text));
            }
        }
        "BYTEA" => {
            if let Ok(v) = row.try_get::<Vec<u8>, _>(ordinal) {
                return (CborValue::Bytes(v), Some(PostgresTypeVariants::Blob));
            }
        }
        _ => {}
    }

    // Fallback: try common types in order
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
        return (CborValue::Text(v), Some(PostgresTypeVariants::Text));
    }

    // Intentional: surface decode failures so missing type handlers are noticed early.
    eprintln!(
        "vantage: failed to decode PostgreSQL column '{}' (type '{}') — returning NULL",
        row.columns()[ordinal].name(),
        type_name,
    );
    (CborValue::Null, Some(PostgresTypeVariants::Null))
}

/// Convert a serde_json::Value to CborValue (used for JSON/JSONB columns).
fn json_value_to_cbor(val: serde_json::Value) -> CborValue {
    match val {
        serde_json::Value::Null => CborValue::Null,
        serde_json::Value::Bool(b) => CborValue::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                CborValue::Integer(i.into())
            } else if let Some(u) = n.as_u64() {
                CborValue::Integer(u.into())
            } else if let Some(f) = n.as_f64() {
                CborValue::Float(f)
            } else {
                CborValue::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => CborValue::Text(s),
        serde_json::Value::Array(arr) => {
            CborValue::Array(arr.into_iter().map(json_value_to_cbor).collect())
        }
        serde_json::Value::Object(map) => CborValue::Map(
            map.into_iter()
                .map(|(k, v)| (CborValue::Text(k), json_value_to_cbor(v)))
                .collect(),
        ),
    }
}
