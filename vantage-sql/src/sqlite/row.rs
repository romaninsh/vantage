//! sqlx rows and bind values for SQLite. Binding follows the variant tag of
//! `AnySqliteType`; reading infers the variant from the column type and keeps
//! the value as CBOR.

use ciborium::Value as CborValue;
use sqlx::sqlite::SqliteRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_types::Record;

use super::types::{AnySqliteType, SqliteTypeVariants};

pub(crate) type SqliteQuery<'q> =
    sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>;

/// Values the binder cannot express as a SQLite parameter come straight
/// from user data, so they surface as errors — never a panic.
pub(crate) fn bind_sqlite_value<'q>(
    query: SqliteQuery<'q>,
    value: &'q AnySqliteType,
) -> vantage_core::Result<SqliteQuery<'q>> {
    let cbor = value.value();
    Ok(match value.type_variant() {
        Some(SqliteTypeVariants::Null) => query.bind(None::<String>),
        None => return bind_by_cbor(query, cbor),
        Some(SqliteTypeVariants::Bool) => match cbor {
            CborValue::Null => query.bind(None::<bool>),
            CborValue::Integer(i) => match i64::try_from(*i) {
                Ok(n) => query.bind(n != 0),
                Err(_) => query.bind(None::<bool>),
            },
            CborValue::Bool(b) => query.bind(*b),
            _ => query.bind(None::<bool>),
        },
        Some(SqliteTypeVariants::Integer) => match cbor {
            CborValue::Null => query.bind(None::<i64>),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok()),
            _ => query.bind(None::<i64>),
        },
        Some(SqliteTypeVariants::Real) => match cbor {
            CborValue::Null => query.bind(None::<f64>),
            CborValue::Float(f) => query.bind(*f),
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok().map(|n| n as f64)),
            _ => query.bind(None::<f64>),
        },
        Some(SqliteTypeVariants::Text) => match cbor {
            CborValue::Null => query.bind(None::<String>),
            CborValue::Text(s) => query.bind(s.as_str()),
            CborValue::Tag(_, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    query.bind(s.as_str())
                } else {
                    query.bind(None::<String>)
                }
            }
            _ => query.bind(None::<String>),
        },
        Some(SqliteTypeVariants::Numeric) => match cbor {
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
        Some(SqliteTypeVariants::Blob) => match cbor {
            CborValue::Null => query.bind(None::<Vec<u8>>),
            CborValue::Bytes(b) => query.bind(b.as_slice()),
            CborValue::Text(s) => query.bind(s.as_bytes()),
            _ => query.bind(None::<Vec<u8>>),
        },
    })
}

/// Bind a CBOR value without type variant — infers from the value itself.
fn bind_by_cbor<'q>(
    query: SqliteQuery<'q>,
    cbor: &'q CborValue,
) -> vantage_core::Result<SqliteQuery<'q>> {
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
        CborValue::Tag(10, inner) => {
            if let CborValue::Text(s) = inner.as_ref() {
                query.bind(s.as_str())
            } else {
                query.bind(None::<String>)
            }
        }
        CborValue::Tag(0 | 100 | 101, inner) => {
            if let CborValue::Text(s) = inner.as_ref() {
                query.bind(s.as_str())
            } else {
                query.bind(None::<String>)
            }
        }
        // Record reference — Tag(8, ["table", id]) or Tag(8, "table:id").
        // SQLite has no reference type, so the reference lowers to its id.
        CborValue::Tag(8, inner) => return bind_reference_id(query, inner),
        other => {
            return Err(vantage_core::error!(
                "cannot bind CBOR value to a SQLite parameter",
                value = describe_cbor(other)
            ));
        }
    })
}

/// Lower a `Tag(8)` record reference to its id and bind that. A numeric
/// id binds as INTEGER so it satisfies INTEGER (and rowid) columns; a
/// non-numeric id binds as TEXT.
fn bind_reference_id<'q>(
    query: SqliteQuery<'q>,
    inner: &'q CborValue,
) -> vantage_core::Result<SqliteQuery<'q>> {
    let bind_id_text = |query: SqliteQuery<'q>, s: &'q str| match s.parse::<i64>() {
        Ok(n) => query.bind(n),
        Err(_) => query.bind(s),
    };
    Ok(match inner {
        // SurrealDB string form "table:id" — everything after the first
        // colon is the id (the whole text if there is no colon).
        CborValue::Text(s) => {
            let id = s.split_once(':').map(|(_, id)| id).unwrap_or(s.as_str());
            bind_id_text(query, id)
        }
        CborValue::Array(parts) if parts.len() == 2 => match &parts[1] {
            CborValue::Integer(i) => query.bind(i64::try_from(*i).ok()),
            CborValue::Text(s) => bind_id_text(query, s.as_str()),
            CborValue::Null => query.bind(None::<String>),
            other => {
                return Err(vantage_core::error!(
                    "record reference id is not a scalar",
                    id = describe_cbor(other)
                ));
            }
        },
        other => {
            return Err(vantage_core::error!(
                "record reference has an unexpected shape",
                value = describe_cbor(other)
            ));
        }
    })
}

/// Compact one-line description of a CBOR value for error context —
/// shapes and types with values truncated, so errors stay readable and
/// don't leak whole records.
fn describe_cbor(v: &CborValue) -> String {
    const MAX_TEXT: usize = 32;
    match v {
        CborValue::Null => "Null".into(),
        CborValue::Bool(b) => format!("Bool({b})"),
        CborValue::Integer(i) => format!("Integer({})", i128::from(*i)),
        CborValue::Float(f) => format!("Float({f})"),
        CborValue::Text(s) if s.len() <= MAX_TEXT => format!("Text({s:?})"),
        CborValue::Text(s) => {
            let cut: String = s.chars().take(MAX_TEXT).collect();
            format!("Text({cut:?}…)")
        }
        CborValue::Bytes(b) => format!("Bytes(len {})", b.len()),
        CborValue::Array(a) => format!("Array(len {})", a.len()),
        CborValue::Map(m) => format!("Map(len {})", m.len()),
        CborValue::Tag(t, inner) => format!("Tag({t}, {})", describe_cbor(inner)),
        _ => "unknown CBOR value".into(),
    }
}

/// `?1=Text ?2=Integer …` — parameter type summary for error context.
/// Types only, never values.
pub(crate) fn describe_param_types(params: &[AnySqliteType]) -> String {
    params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let ty = match p.type_variant() {
                Some(v) => format!("{v:?}"),
                None => match p.value() {
                    CborValue::Null => "Null".into(),
                    CborValue::Bool(_) => "Bool".into(),
                    CborValue::Integer(_) => "Integer".into(),
                    CborValue::Float(_) => "Real".into(),
                    CborValue::Text(_) => "Text".into(),
                    CborValue::Bytes(_) => "Blob".into(),
                    CborValue::Tag(t, _) => format!("Tag({t})"),
                    _ => "?".into(),
                },
            };
            format!("?{}={}", i + 1, ty)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert a `SqliteRow` to `Record<AnySqliteType>`.
///
/// Each value is stored as CBOR with the type variant inferred from the
/// SQLite column type, preserving full type fidelity.
pub(crate) fn row_to_record(row: &SqliteRow) -> Record<AnySqliteType> {
    let mut record = Record::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let declared_type = col.type_info().name();
        let (cbor, variant) = sqlite_column_to_cbor(row, col.ordinal(), declared_type);
        let value = match variant {
            Some(v) => AnySqliteType::with_variant(cbor, v),
            None => AnySqliteType::untyped(cbor),
        };
        record.insert(name, value);
    }
    record
}

/// Read a single column from a SQLite row as CborValue, returning both the
/// value and the detected type variant.
fn sqlite_column_to_cbor(
    row: &SqliteRow,
    ordinal: usize,
    declared_type: &str,
) -> (CborValue, Option<SqliteTypeVariants>) {
    use sqlx::ValueRef;

    if row
        .try_get_raw(ordinal)
        .map(|v| v.is_null())
        .unwrap_or(true)
    {
        return (CborValue::Null, None);
    }

    let dt = declared_type.to_uppercase();

    // Boolean — SQLite stores as 0/1 INTEGER
    if (dt == "BOOLEAN" || dt == "BOOL")
        && let Ok(v) = row.try_get::<bool, _>(ordinal)
    {
        return (CborValue::Bool(v), Some(SqliteTypeVariants::Bool));
    }

    if dt == "BLOB"
        && let Ok(v) = row.try_get::<Vec<u8>, _>(ordinal)
    {
        return (CborValue::Bytes(v), Some(SqliteTypeVariants::Blob));
    }

    if let Ok(v) = row.try_get::<i64, _>(ordinal) {
        return (
            CborValue::Integer(v.into()),
            Some(SqliteTypeVariants::Integer),
        );
    }
    if let Ok(v) = row.try_get::<f64, _>(ordinal) {
        return (CborValue::Float(v), Some(SqliteTypeVariants::Real));
    }
    if let Ok(v) = row.try_get::<String, _>(ordinal) {
        // Untyped — allows try_get to attempt parsing as DateTime, Decimal, etc.
        return (CborValue::Text(v), None);
    }

    eprintln!(
        "vantage: failed to decode SQLite column '{}' (type '{}') — returning NULL",
        row.columns()[ordinal].name(),
        declared_type,
    );
    (CborValue::Null, None)
}
