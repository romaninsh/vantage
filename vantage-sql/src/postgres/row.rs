//! Helpers for converting between sqlx rows/values and vantage types.
//!
//! **Writing** (bind): Takes `AnyPostgresType` with variant tags -- the variant
//! tells us exactly how to bind each value to sqlx.
//!
//! **Reading** (row): Returns `Record<AnyPostgresType>` with `type_variant: None`.
//! The values are marker-less -- `try_get` will attempt conversion without
//! variant enforcement, and struct deserialization validates the actual types.

use serde_json::Value as JsonValue;
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_types::Record;

use super::types::{AnyPostgresType, PostgresTypeVariants};

/// Bind an AnyPostgresType to a sqlx query. Uses the variant tag to pick
/// the right sqlx bind type -- no guessing from the JSON number format.
pub(crate) fn bind_postgres_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: &'q AnyPostgresType,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    let json = value.value();
    match value.type_variant() {
        Some(PostgresTypeVariants::Null) => query.bind(None::<String>),
        // Untyped values (from deferred results, database reads) -- infer from JSON
        None => bind_by_json(query, json),
        Some(PostgresTypeVariants::Bool) => match json {
            JsonValue::Null => query.bind(None::<bool>),
            JsonValue::Bool(b) => query.bind(*b),
            JsonValue::Number(n) => match n.as_i64() {
                Some(i) => query.bind(i != 0),
                None => query.bind(None::<bool>),
            },
            _ => query.bind(None::<bool>),
        },
        Some(PostgresTypeVariants::Int2) => match json {
            JsonValue::Null => query.bind(None::<i16>),
            JsonValue::Number(n) => query.bind(n.as_i64().map(|i| i as i16)),
            _ => query.bind(None::<i16>),
        },
        Some(PostgresTypeVariants::Int4) => match json {
            JsonValue::Null => query.bind(None::<i32>),
            JsonValue::Number(n) => query.bind(n.as_i64().map(|i| i as i32)),
            _ => query.bind(None::<i32>),
        },
        Some(PostgresTypeVariants::Int8) => match json {
            JsonValue::Null => query.bind(None::<i64>),
            JsonValue::Number(n) => query.bind(n.as_i64()),
            _ => query.bind(None::<i64>),
        },
        Some(PostgresTypeVariants::Float4) => match json {
            JsonValue::Null => query.bind(None::<f32>),
            JsonValue::Number(n) => query.bind(n.as_f64().map(|f| f as f32)),
            _ => query.bind(None::<f32>),
        },
        Some(PostgresTypeVariants::Float8) => match json {
            JsonValue::Null => query.bind(None::<f64>),
            JsonValue::Number(n) => query.bind(n.as_f64()),
            _ => query.bind(None::<f64>),
        },
        Some(PostgresTypeVariants::Text) => match json {
            JsonValue::Null => query.bind(None::<String>),
            JsonValue::String(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
    }
}

/// Bind a JSON value without type variant -- infers the bind type from the value itself.
fn bind_by_json<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    json: &'q JsonValue,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match json {
        JsonValue::Null => query.bind(None::<String>),
        JsonValue::Bool(b) => query.bind(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                query.bind(i)
            } else if let Some(f) = n.as_f64() {
                query.bind(f)
            } else {
                query.bind(n.to_string())
            }
        }
        JsonValue::String(s) => query.bind(s.as_str()),
        other => query.bind(other.to_string()),
    }
}

/// Convert a PgRow to Record<AnyPostgresType>.
///
/// Each value has `type_variant: None` -- the database doesn't preserve our
/// type markers. This means `try_get` on these values bypasses variant
/// checking and just attempts the conversion.
pub(crate) fn row_to_record(row: &PgRow) -> Record<AnyPostgresType> {
    let mut record = Record::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let type_name = col.type_info().name();
        let json = pg_column_to_json(row, col.ordinal(), type_name);
        let value = AnyPostgresType::untyped(json);
        record.insert(name, value);
    }
    record
}

/// Read a single column from a PostgreSQL row as JsonValue.
/// Uses the declared column type to pick the right extraction method.
fn pg_column_to_json(row: &PgRow, ordinal: usize, type_name: &str) -> JsonValue {
    use sqlx::ValueRef;

    if row
        .try_get_raw(ordinal)
        .map(|v| v.is_null())
        .unwrap_or(true)
    {
        return JsonValue::Null;
    }

    match type_name {
        "BOOL" => {
            if let Ok(v) = row.try_get::<bool, _>(ordinal) {
                return JsonValue::Bool(v);
            }
        }
        "INT2" | "SMALLINT" | "SMALLSERIAL" => {
            if let Ok(v) = row.try_get::<i16, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "INT4" | "INT" | "INTEGER" | "SERIAL" => {
            if let Ok(v) = row.try_get::<i32, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "INT8" | "BIGINT" | "BIGSERIAL" => {
            if let Ok(v) = row.try_get::<i64, _>(ordinal) {
                return JsonValue::Number(v.into());
            }
        }
        "FLOAT4" | "REAL" => {
            if let Ok(v) = row.try_get::<f32, _>(ordinal)
                && let Some(n) = serde_json::Number::from_f64(v as f64)
            {
                return JsonValue::Number(n);
            }
        }
        "FLOAT8" | "DOUBLE PRECISION" => {
            if let Ok(v) = row.try_get::<f64, _>(ordinal)
                && let Some(n) = serde_json::Number::from_f64(v)
            {
                return JsonValue::Number(n);
            }
        }
        "NUMERIC" | "DECIMAL" => {
            // PostgreSQL NUMERIC: decode via rust_decimal, then convert
            // to JSON number. Integer values are exact; decimals go through
            // f64 which may lose precision for very large or high-scale values.
            if let Ok(v) = row.try_get::<rust_decimal::Decimal, _>(ordinal) {
                use rust_decimal::prelude::ToPrimitive;
                if v.scale() == 0
                    && let Some(i) = v.to_i64()
                {
                    return JsonValue::Number(i.into());
                }
                if let Some(f) = v.to_f64()
                    && let Some(n) = serde_json::Number::from_f64(f)
                {
                    return JsonValue::Number(n);
                }
            }
        }
        _ => {}
    }

    // Fallback: try common types in order
    if let Ok(v) = row.try_get::<bool, _>(ordinal) {
        return JsonValue::Bool(v);
    }
    if let Ok(v) = row.try_get::<i64, _>(ordinal) {
        return JsonValue::Number(v.into());
    }
    if let Ok(v) = row.try_get::<i32, _>(ordinal) {
        return JsonValue::Number((v as i64).into());
    }
    if let Ok(v) = row.try_get::<f64, _>(ordinal)
        && let Some(n) = serde_json::Number::from_f64(v)
    {
        return JsonValue::Number(n);
    }
    if let Ok(v) = row.try_get::<String, _>(ordinal) {
        return JsonValue::String(v);
    }

    JsonValue::Null
}
