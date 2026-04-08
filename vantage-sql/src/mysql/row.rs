//! Helpers for converting between sqlx rows/values and vantage types.
//!
//! **Writing** (bind): Takes `AnyMysqlType` with variant tags -- the variant
//! tells us exactly how to bind each value to sqlx.
//!
//! **Reading** (row): Returns `Record<AnyMysqlType>` with `type_variant: None`.

use serde_json::Value as JsonValue;
use sqlx::mysql::MySqlRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_types::Record;

use super::types::{AnyMysqlType, MysqlTypeVariants};

/// Bind an AnyMysqlType to a sqlx query.
pub(crate) fn bind_mysql_value<'q>(
    query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    value: &'q AnyMysqlType,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    let json = value.value();
    match value.type_variant() {
        Some(MysqlTypeVariants::Null) => query.bind(None::<String>),
        None => bind_by_json(query, json),
        Some(MysqlTypeVariants::Bool) => match json {
            JsonValue::Null => query.bind(None::<bool>),
            JsonValue::Bool(b) => query.bind(*b),
            JsonValue::Number(n) => match n.as_i64() {
                Some(i) => query.bind(i != 0),
                None => query.bind(None::<bool>),
            },
            _ => query.bind(None::<bool>),
        },
        Some(MysqlTypeVariants::Int2) => match json {
            JsonValue::Null => query.bind(None::<i16>),
            JsonValue::Number(n) => query.bind(n.as_i64().map(|i| i as i16)),
            _ => query.bind(None::<i16>),
        },
        Some(MysqlTypeVariants::Int4) => match json {
            JsonValue::Null => query.bind(None::<i32>),
            JsonValue::Number(n) => query.bind(n.as_i64().map(|i| i as i32)),
            _ => query.bind(None::<i32>),
        },
        Some(MysqlTypeVariants::Int8) => match json {
            JsonValue::Null => query.bind(None::<i64>),
            JsonValue::Number(n) => query.bind(n.as_i64()),
            _ => query.bind(None::<i64>),
        },
        Some(MysqlTypeVariants::Float4) => match json {
            JsonValue::Null => query.bind(None::<f32>),
            JsonValue::Number(n) => query.bind(n.as_f64().map(|f| f as f32)),
            _ => query.bind(None::<f32>),
        },
        Some(MysqlTypeVariants::Float8) => match json {
            JsonValue::Null => query.bind(None::<f64>),
            JsonValue::Number(n) => query.bind(n.as_f64()),
            _ => query.bind(None::<f64>),
        },
        Some(MysqlTypeVariants::Text) => match json {
            JsonValue::Null => query.bind(None::<String>),
            JsonValue::String(s) => query.bind(s.as_str()),
            _ => query.bind(None::<String>),
        },
    }
}

/// Bind a JSON value without type variant -- infers from the value itself.
fn bind_by_json<'q>(
    query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    json: &'q JsonValue,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
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

/// Convert a MySqlRow to Record<AnyMysqlType>.
pub(crate) fn row_to_record(row: &MySqlRow) -> Record<AnyMysqlType> {
    let mut record = Record::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let type_name = col.type_info().name();
        let json = mysql_column_to_json(row, col.ordinal(), type_name);
        let value = AnyMysqlType::untyped(json);
        record.insert(name, value);
    }
    record
}

/// Read a single column from a MySQL row as JsonValue.
fn mysql_column_to_json(row: &MySqlRow, ordinal: usize, type_name: &str) -> JsonValue {
    use sqlx::ValueRef;

    if row
        .try_get_raw(ordinal)
        .map(|v| v.is_null())
        .unwrap_or(true)
    {
        return JsonValue::Null;
    }

    match type_name {
        "BOOLEAN" | "BOOL" => {
            // MySQL BOOLEAN is TINYINT(1). sqlx decodes as i8.
            if let Ok(v) = row.try_get::<i8, _>(ordinal) {
                return JsonValue::Bool(v != 0);
            }
            if let Ok(v) = row.try_get::<bool, _>(ordinal) {
                return JsonValue::Bool(v);
            }
        }
        "TINYINT" => {
            if let Ok(v) = row.try_get::<i8, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "SMALLINT" => {
            if let Ok(v) = row.try_get::<i16, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "INT" | "INTEGER" | "MEDIUMINT" => {
            if let Ok(v) = row.try_get::<i32, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "BIGINT" => {
            if let Ok(v) = row.try_get::<i64, _>(ordinal) {
                return JsonValue::Number(v.into());
            }
        }
        "BIGINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u64, _>(ordinal) {
                return JsonValue::Number(v.into());
            }
        }
        "INT UNSIGNED" | "MEDIUMINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u32, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "SMALLINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u16, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "TINYINT UNSIGNED" => {
            if let Ok(v) = row.try_get::<u8, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "JSON" => {
            if let Ok(v) = row.try_get::<serde_json::Value, _>(ordinal) {
                return v;
            }
        }
        "FLOAT" => {
            if let Ok(v) = row.try_get::<f32, _>(ordinal)
                && let Some(n) = serde_json::Number::from_f64(v as f64)
            {
                return JsonValue::Number(n);
            }
        }
        "DOUBLE" => {
            if let Ok(v) = row.try_get::<f64, _>(ordinal)
                && let Some(n) = serde_json::Number::from_f64(v)
            {
                return JsonValue::Number(n);
            }
        }
        "DECIMAL" | "NUMERIC" | "NEWDECIMAL" => {
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
            // Fallback: try as string and parse
            if let Ok(v) = row.try_get::<String, _>(ordinal) {
                if let Ok(i) = v.parse::<i64>() {
                    return JsonValue::Number(i.into());
                }
                if let Ok(f) = v.parse::<f64>()
                    && let Some(n) = serde_json::Number::from_f64(f)
                {
                    return JsonValue::Number(n);
                }
                return JsonValue::String(v);
            }
        }
        "TEXT" | "TINYTEXT" | "MEDIUMTEXT" | "LONGTEXT" | "VARCHAR" | "CHAR" | "ENUM" | "SET" => {
            if let Ok(v) = row.try_get::<String, _>(ordinal) {
                return JsonValue::String(v);
            }
        }
        "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" => {
            // sqlx decodes BLOB types as Vec<u8>; try String first, then bytes
            if let Ok(v) = row.try_get::<String, _>(ordinal) {
                return JsonValue::String(v);
            }
            if let Ok(v) = row.try_get::<Vec<u8>, _>(ordinal) {
                return JsonValue::String(String::from_utf8_lossy(&v).into_owned());
            }
        }
        "TIME" => {
            if let Ok(v) = row.try_get::<chrono::NaiveTime, _>(ordinal) {
                return JsonValue::String(v.format("%H:%M:%S").to_string());
            }
        }
        "DATE" => {
            if let Ok(v) = row.try_get::<chrono::NaiveDate, _>(ordinal) {
                return JsonValue::String(v.format("%Y-%m-%d").to_string());
            }
        }
        "DATETIME" => {
            if let Ok(v) = row.try_get::<chrono::NaiveDateTime, _>(ordinal) {
                return JsonValue::String(v.format("%Y-%m-%d %H:%M:%S%.f").to_string());
            }
        }
        "TIMESTAMP" | "TIMESTAMP(6)" => {
            if let Ok(v) = row.try_get::<chrono::DateTime<chrono::Utc>, _>(ordinal) {
                return JsonValue::String(v.format("%Y-%m-%d %H:%M:%S%.f").to_string());
            }
        }
        "YEAR" => {
            if let Ok(v) = row.try_get::<u16, _>(ordinal) {
                return JsonValue::Number((v as i64).into());
            }
        }
        "BIT" => {
            if let Ok(v) = row.try_get::<u64, _>(ordinal) {
                return JsonValue::Number(v.into());
            }
        }
        _ => {}
    }

    // Fallback: try common types in order
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

    eprintln!(
        "vantage: failed to decode MySQL column '{}' (type '{}') — returning NULL",
        row.columns()[ordinal].name(),
        type_name,
    );
    JsonValue::Null
}
