//! Helpers for converting between sqlx rows/values and vantage types.
//!
//! **Writing** (bind): Takes `AnySqliteType` with variant tags — the variant
//! tells us exactly how to bind each value to sqlx.
//!
//! **Reading** (row): Returns `Record<AnySqliteType>` with variant inferred from
//! the SQLite column type. Values are stored as `ciborium::Value` (CBOR) for
//! lossless type preservation.

use ciborium::Value as CborValue;
use sqlx::sqlite::SqliteRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_types::Record;

use super::types::{AnySqliteType, SqliteTypeVariants};

/// Bind an AnySqliteType to a sqlx query. Uses the variant tag to pick
/// the right sqlx bind type — no guessing from the CBOR value format.
pub(crate) fn bind_sqlite_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    value: &'q AnySqliteType,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    let cbor = value.value();
    match value.type_variant() {
        Some(SqliteTypeVariants::Null) => query.bind(None::<String>),
        None => bind_by_cbor(query, cbor),
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
    }
}

/// Bind a CBOR value without type variant — infers from the value itself.
fn bind_by_cbor<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    cbor: &'q CborValue,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
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
        other => panic!(
            "bind_by_cbor: unexpected CBOR value type {:?} — this is a bug upstream",
            other
        ),
    }
}

/// Convert a SqliteRow to Record<AnySqliteType>.
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

    // BLOB
    if dt == "BLOB"
        && let Ok(v) = row.try_get::<Vec<u8>, _>(ordinal)
    {
        return (CborValue::Bytes(v), Some(SqliteTypeVariants::Blob));
    }

    // Fallback: try common types in order
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
        return (CborValue::Text(v), Some(SqliteTypeVariants::Text));
    }

    eprintln!(
        "vantage: failed to decode SQLite column '{}' (type '{}') — returning NULL",
        row.columns()[ordinal].name(),
        declared_type,
    );
    (CborValue::Null, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteDB;
    use serde::{Deserialize, Serialize};
    use serde_json::Value as JsonValue;
    use vantage_types::TryFromRecord;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Product {
        name: String,
        price: i64,
        is_deleted: bool,
    }

    #[tokio::test]
    async fn test_row_to_record_try_get() {
        let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
            "CREATE TABLE t (
                name TEXT NOT NULL,
                score INTEGER NOT NULL,
                ratio REAL NOT NULL,
                active BOOLEAN NOT NULL,
                note TEXT
            )",
        )
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query("INSERT INTO t VALUES ('Alice', 42, 3.15, 1, NULL)")
            .execute(db.pool())
            .await
            .unwrap();

        let rows: Vec<SqliteRow> = sqlx::query("SELECT * FROM t")
            .fetch_all(db.pool())
            .await
            .unwrap();

        let record = row_to_record(&rows[0]);

        assert_eq!(
            record["name"].try_get::<String>(),
            Some("Alice".to_string())
        );
        assert_eq!(record["score"].try_get::<i64>(), Some(42));
        assert!((record["ratio"].try_get::<f64>().unwrap() - 3.15).abs() < f64::EPSILON);
        assert_eq!(record["active"].try_get::<bool>(), Some(true));

        // NULL column — try_get on Option works
        assert_eq!(record["note"].try_get::<Option<String>>(), Some(None));
    }

    #[tokio::test]
    async fn test_row_to_record_into_struct() {
        let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
            "CREATE TABLE products (
                name TEXT NOT NULL,
                price INTEGER NOT NULL,
                is_deleted BOOLEAN NOT NULL
            )",
        )
        .execute(db.pool())
        .await
        .unwrap();

        sqlx::query("INSERT INTO products VALUES ('Cupcake', 120, 0)")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO products VALUES ('Tart', 220, 1)")
            .execute(db.pool())
            .await
            .unwrap();

        let rows: Vec<SqliteRow> = sqlx::query("SELECT * FROM products ORDER BY price")
            .fetch_all(db.pool())
            .await
            .unwrap();

        // Record<AnySqliteType> → Record<JsonValue> via JSON bridge → struct via serde
        let products: Vec<Product> = rows
            .iter()
            .map(|row| {
                let record = row_to_record(row);
                let json_record: Record<JsonValue> = record
                    .into_iter()
                    .map(|(k, v)| (k, JsonValue::from(v)))
                    .collect();
                Product::from_record(json_record).unwrap()
            })
            .collect();

        assert_eq!(products.len(), 2);
        assert_eq!(
            products[0],
            Product {
                name: "Cupcake".into(),
                price: 120,
                is_deleted: false
            }
        );
        assert_eq!(
            products[1],
            Product {
                name: "Tart".into(),
                price: 220,
                is_deleted: true
            }
        );
    }

    #[tokio::test]
    async fn test_bind_sqlite_value_types() {
        let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

        sqlx::query("CREATE TABLE bind_test (val TEXT)")
            .execute(db.pool())
            .await
            .unwrap();

        let text_val = AnySqliteType::new("hello".to_string());
        let mut q = sqlx::query("INSERT INTO bind_test VALUES (?)");
        q = bind_sqlite_value(q, &text_val);
        q.execute(db.pool()).await.unwrap();

        let rows: Vec<SqliteRow> = sqlx::query("SELECT val FROM bind_test")
            .fetch_all(db.pool())
            .await
            .unwrap();
        let record = row_to_record(&rows[0]);
        assert_eq!(record["val"].try_get::<String>(), Some("hello".to_string()));
    }

    #[tokio::test]
    async fn test_bind_integer_and_bool() {
        let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

        sqlx::query("CREATE TABLE ib (i INTEGER, b BOOLEAN)")
            .execute(db.pool())
            .await
            .unwrap();

        let int_val = AnySqliteType::new(42i64);
        let bool_val = AnySqliteType::new(true);

        let mut q = sqlx::query("INSERT INTO ib VALUES (?, ?)");
        q = bind_sqlite_value(q, &int_val);
        q = bind_sqlite_value(q, &bool_val);
        q.execute(db.pool()).await.unwrap();

        let rows: Vec<SqliteRow> = sqlx::query("SELECT * FROM ib")
            .fetch_all(db.pool())
            .await
            .unwrap();
        let record = row_to_record(&rows[0]);
        assert_eq!(record["i"].try_get::<i64>(), Some(42));
        assert_eq!(record["b"].try_get::<bool>(), Some(true));
    }
}
