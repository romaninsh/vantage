//! Helpers for converting between sqlx rows/values and vantage types.
//!
//! **Writing** (bind): Takes `AnySqliteType` with variant tags — the variant
//! tells us exactly how to bind each value to sqlx.
//!
//! **Reading** (row): Returns `Record<AnySqliteType>` with `type_variant: None`.
//! The values are marker-less — `try_get` will attempt conversion without
//! variant enforcement, and struct deserialization validates the actual types.

use serde_json::Value as JsonValue;
use sqlx::sqlite::SqliteRow;
use sqlx::{Column, Row, TypeInfo};
use vantage_types::Record;

use super::types::{AnySqliteType, SqliteTypeVariants};

/// Bind an AnySqliteType to a sqlx query. Uses the variant tag to pick
/// the right sqlx bind type — no guessing from the JSON number format.
pub(crate) fn bind_sqlite_value<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    value: &'q AnySqliteType,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    let json = value.value();
    match value.type_variant() {
        Some(SqliteTypeVariants::Null) => query.bind(None::<String>),
        // Untyped values (from deferred results, database reads) — infer from JSON
        None => bind_by_json(query, json),
        Some(SqliteTypeVariants::Bool) => {
            match json {
                JsonValue::Number(n) => query.bind(n.as_i64().unwrap_or(0) != 0),
                JsonValue::Bool(b) => query.bind(*b),
                _ => query.bind(false),
            }
        }
        Some(SqliteTypeVariants::Integer) => {
            match json {
                JsonValue::Number(n) => query.bind(n.as_i64().unwrap_or(0)),
                _ => query.bind(None::<String>),
            }
        }
        Some(SqliteTypeVariants::Real) => {
            let f = json.as_f64().unwrap_or(0.0);
            query.bind(f)
        }
        Some(SqliteTypeVariants::Text) => {
            let s = json.as_str().unwrap_or("");
            query.bind(s)
        }
        Some(SqliteTypeVariants::Numeric) => {
            let s = json
                .as_object()
                .and_then(|o| o.get("numeric"))
                .and_then(|v| v.as_str())
                .unwrap_or("0");
            query.bind(s)
        }
        Some(SqliteTypeVariants::Blob) => {
            let s = json
                .as_object()
                .and_then(|o| o.get("blob"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            query.bind(s)
        }
    }
}

/// Bind a JSON value without type variant — infers the bind type from the value itself.
/// Used for untyped values (deferred results, database reads).
fn bind_by_json<'q>(
    query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    json: &'q JsonValue,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
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

/// Convert a SqliteRow to Record<AnySqliteType>.
///
/// Each value has `type_variant: None` — the database doesn't preserve our
/// type markers. This means `try_get` on these values bypasses variant
/// checking and just attempts the conversion, which is the right behavior
/// for data coming back from the database.
pub(crate) fn row_to_record(row: &SqliteRow) -> Record<AnySqliteType> {
    let mut record = Record::new();
    for col in row.columns() {
        let name = col.name().to_string();
        let declared_type = col.type_info().name();
        let json = sqlite_column_to_json(row, col.ordinal(), declared_type);
        // Wrap as AnySqliteType with type_variant: None — we intentionally
        // don't assign variants to values coming from the database, so that
        // try_get is permissive and attempts any conversion.
        let value = AnySqliteType::untyped(json);
        record.insert(name, value);
    }
    record
}

/// Read a single column from a SQLite row as JsonValue.
/// Uses the declared column type to disambiguate (e.g., INTEGER vs BOOLEAN).
fn sqlite_column_to_json(row: &SqliteRow, ordinal: usize, declared_type: &str) -> JsonValue {
    use sqlx::ValueRef;

    if row.try_get_raw(ordinal).map(|v| v.is_null()).unwrap_or(true) {
        return JsonValue::Null;
    }

    let dt = declared_type.to_uppercase();
    if dt == "BOOLEAN" || dt == "BOOL" {
        if let Ok(v) = row.try_get::<bool, _>(ordinal) {
            return JsonValue::Bool(v);
        }
    }

    if let Ok(v) = row.try_get::<i64, _>(ordinal) {
        return JsonValue::Number(v.into());
    }
    if let Ok(v) = row.try_get::<f64, _>(ordinal) {
        if let Some(n) = serde_json::Number::from_f64(v) {
            return JsonValue::Number(n);
        }
    }
    if let Ok(v) = row.try_get::<String, _>(ordinal) {
        return JsonValue::String(v);
    }

    JsonValue::Null
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqliteDB;
    use serde::{Deserialize, Serialize};
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

        sqlx::query("INSERT INTO t VALUES ('Alice', 42, 3.14, 1, NULL)")
            .execute(db.pool())
            .await
            .unwrap();

        let rows: Vec<SqliteRow> = sqlx::query("SELECT * FROM t")
            .fetch_all(db.pool())
            .await
            .unwrap();

        let record = row_to_record(&rows[0]);

        // Values have type_variant from from_json detection, but try_get
        // is permissive — it attempts conversion regardless
        assert_eq!(record["name"].try_get::<String>(), Some("Alice".to_string()));
        assert_eq!(record["score"].try_get::<i64>(), Some(42));
        assert!((record["ratio"].try_get::<f64>().unwrap() - 3.14).abs() < f64::EPSILON);
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

        // Record<AnySqliteType> → Record<JsonValue> → struct via serde
        let products: Vec<Product> = rows
            .iter()
            .map(|row| {
                let record = row_to_record(row);
                let json_record: Record<JsonValue> = record
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect();
                Product::from_record(json_record).unwrap()
            })
            .collect();

        assert_eq!(products.len(), 2);
        assert_eq!(products[0], Product { name: "Cupcake".into(), price: 120, is_deleted: false });
        assert_eq!(products[1], Product { name: "Tart".into(), price: 220, is_deleted: true });
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
