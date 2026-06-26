//! Auto-stamping of audit timestamps on write.
//!
//! [`Table::with_timestamps`] fills `created_at`/`updated_at` columns from the
//! wall clock using the before-write hook machinery (see [`crate::table::hooks`]):
//! `created_at` once, when a row is first inserted and only if the caller left it
//! empty; `updated_at` on every insert and update. Values are RFC 3339 UTC
//! strings (e.g. `2026-06-25T12:00:00Z`).
//!
//! Like [`crate::table::id_generator`], it leans on the same `From<String>`
//! value bound, so it works against any backend whose value type can carry a
//! string — the column itself can be plain `TEXT`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use vantage_core::Result;
use vantage_types::{EmptyEntity, Entity, InvariantValue, Record};

use crate::table::{BeforeFn, Hook, Phase, Table};
use crate::traits::table_source::TableSource;

/// Which columns the audit stamps write. Defaults to `created_at` / `updated_at`.
#[derive(Clone, Debug)]
pub struct Timestamps {
    created_column: String,
    updated_column: String,
}

impl Default for Timestamps {
    fn default() -> Self {
        Self {
            created_column: "created_at".to_string(),
            updated_column: "updated_at".to_string(),
        }
    }
}

impl Timestamps {
    /// The default `created_at` / `updated_at` pair.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the created-timestamp column name.
    pub fn created(mut self, column: impl Into<String>) -> Self {
        self.created_column = column.into();
        self
    }

    /// Override the updated-timestamp column name.
    pub fn updated(mut self, column: impl Into<String>) -> Self {
        self.updated_column = column.into();
        self
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue + From<String>,
{
    /// Stamp `created_at` on insert and `updated_at` on every write, using the
    /// default column names.
    ///
    /// `created_at` is set only when the inserted record carries none (a
    /// caller-supplied value is kept, so a backfilled import keeps its real
    /// timestamps); `updated_at` is always (re)written. Both are RFC 3339 UTC
    /// strings. The columns need only be nullable `TEXT`.
    pub fn with_timestamps(self) -> Self {
        self.with_audit(Timestamps::default())
    }

    /// [`Self::with_timestamps`] with custom column names.
    pub fn with_audit(self, columns: Timestamps) -> Self {
        self.with_hook(Hook::BeforeInsert(
            Phase::Populate,
            created_hook::<T>(columns.created_column),
        ))
        .with_hook(Hook::BeforeSave(
            Phase::Populate,
            updated_hook::<T>(columns.updated_column),
        ))
    }
}

/// Before-insert hook: set `column` to the current time only if it is absent or
/// null (an explicit value survives).
fn created_hook<T: TableSource>(column: String) -> BeforeFn<T>
where
    T::Value: InvariantValue + From<String>,
{
    Arc::new(
        move |rec: &mut Record<T::Value>,
              _table: &Table<T, EmptyEntity>|
              -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let column = column.clone();
            Box::pin(async move {
                let absent_or_null = match rec.get(column.as_str()) {
                    None => true,
                    Some(v) => v.is_null(),
                };
                if absent_or_null {
                    rec.insert(column, T::Value::from(now_rfc3339()));
                }
                Ok(())
            })
        },
    )
}

/// Before-save hook (insert + update): always set `column` to the current time.
fn updated_hook<T: TableSource>(column: String) -> BeforeFn<T>
where
    T::Value: InvariantValue + From<String>,
{
    Arc::new(
        move |rec: &mut Record<T::Value>,
              _table: &Table<T, EmptyEntity>|
              -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let column = column.clone();
            Box::pin(async move {
                rec.insert(column, T::Value::from(now_rfc3339()));
                Ok(())
            })
        },
    )
}

/// Current UTC time as a second-precision RFC 3339 string (`…Z`).
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use serde_json::json;
    use vantage_dataset::prelude::{
        InsertableValueSet, ReadableValueSet, WritableValueSet,
    };

    type MockTable = Table<MockTableSource, EmptyEntity>;

    fn looks_like_timestamp(v: &serde_json::Value) -> bool {
        v.as_str()
            .is_some_and(|s| s.len() >= 20 && s.ends_with('Z') && s.contains('T'))
    }

    #[tokio::test]
    async fn stamps_created_and_updated_on_insert() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = MockTable::new("t", src).with_timestamps();

        let id = table
            .insert_return_id_value(&Record::from(json!({"id": "1", "name": "a"})))
            .await
            .unwrap();
        let row = table.get_value(id).await.unwrap().unwrap();
        assert!(looks_like_timestamp(&row["created_at"]), "{row:?}");
        assert!(looks_like_timestamp(&row["updated_at"]), "{row:?}");
    }

    #[tokio::test]
    async fn keeps_caller_supplied_created_at() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = MockTable::new("t", src).with_timestamps();

        let id = table
            .insert_return_id_value(&Record::from(
                json!({"id": "1", "name": "a", "created_at": "2000-01-01T00:00:00Z"}),
            ))
            .await
            .unwrap();
        let row = table.get_value(id).await.unwrap().unwrap();
        // The explicit created_at survives; updated_at is still stamped fresh.
        assert_eq!(row["created_at"], json!("2000-01-01T00:00:00Z"));
        assert!(looks_like_timestamp(&row["updated_at"]));
    }

    #[tokio::test]
    async fn updates_only_updated_at_on_patch() {
        let src = MockTableSource::new()
            .with_data(
                "t",
                vec![json!({"id": "1", "name": "a", "created_at": "2000-01-01T00:00:00Z"})],
            )
            .await;
        let table = MockTable::new("t", src).with_timestamps();

        table
            .patch_value("1", &Record::from(json!({"name": "b"})))
            .await
            .unwrap();
        let row = table.get_value("1".to_string()).await.unwrap().unwrap();
        // created_at untouched by the update; updated_at freshly stamped.
        assert_eq!(row["created_at"], json!("2000-01-01T00:00:00Z"));
        assert!(looks_like_timestamp(&row["updated_at"]));
        assert_eq!(row["name"], json!("b"));
    }

    #[tokio::test]
    async fn custom_column_names() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = MockTable::new("t", src)
            .with_audit(Timestamps::new().created("inserted").updated("touched"));

        let id = table
            .insert_return_id_value(&Record::from(json!({"id": "1"})))
            .await
            .unwrap();
        let row = table.get_value(id).await.unwrap().unwrap();
        assert!(looks_like_timestamp(&row["inserted"]));
        assert!(looks_like_timestamp(&row["touched"]));
    }
}
