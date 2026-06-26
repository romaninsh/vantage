//! Auto-generation of record ids on insert.
//!
//! Some backends supply a primary key themselves (a SQL `DEFAULT`/sequence, an
//! in-memory store's own uuid). When the backend does not — a bare
//! `TEXT PRIMARY KEY`, a REST resource that expects the client to mint the id —
//! [`Table::with_generated_id`] fills the id column on insert using the existing
//! before-write hook machinery (see [`crate::table::hooks`]).
//!
//! It fills the id only when the inserted record carries none (the field is
//! absent or null), and only on insert: `patch`/`replace`/`update`/`upsert`
//! never touch the id. Because a present id is always kept, an id the caller
//! assigned once survives a retried insert — generation stays idempotent.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use vantage_core::Result;
use vantage_types::{EmptyEntity, Entity, InvariantValue, Record};

use crate::table::{BeforeFn, Hook, Phase, Table};
use crate::traits::column_like::ColumnLike;
use crate::traits::table_source::TableSource;

/// How a missing id is minted when a new record is inserted.
///
/// `UuidV7` is the recommended choice: it is time-ordered (RFC 9562), so ids
/// sort by creation time and cluster together in a primary-key index instead of
/// scattering it the way the purely-random `UuidV4` does.
#[derive(Clone)]
pub enum IdGenerator {
    /// Time-ordered UUID (v7). Sortable and index-friendly — the recommended
    /// default for new tables.
    UuidV7,
    /// Random UUID (v4).
    UuidV4,
    /// Any caller-supplied scheme (ULID, nanoid, a counter, …).
    Custom(Arc<dyn Fn() -> String + Send + Sync>),
}

impl IdGenerator {
    /// A custom generator from a closure.
    pub fn custom(f: impl Fn() -> String + Send + Sync + 'static) -> Self {
        IdGenerator::Custom(Arc::new(f))
    }

    /// Produce one fresh id string.
    pub fn generate(&self) -> String {
        match self {
            IdGenerator::UuidV7 => uuid::Uuid::now_v7().to_string(),
            IdGenerator::UuidV4 => uuid::Uuid::new_v4().to_string(),
            IdGenerator::Custom(f) => f(),
        }
    }
}

impl std::fmt::Debug for IdGenerator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            IdGenerator::UuidV7 => "UuidV7",
            IdGenerator::UuidV4 => "UuidV4",
            IdGenerator::Custom(_) => "Custom",
        };
        f.debug_tuple("IdGenerator").field(&name).finish()
    }
}

impl<T: TableSource, E: Entity<T::Value>> Table<T, E>
where
    T::Value: InvariantValue + From<String>,
{
    /// Fill the id column with a generated value whenever a new row is inserted
    /// without one.
    ///
    /// Only insert is affected, and only when the record's id field is absent or
    /// null — a caller-supplied id is always kept, so a retried insert that
    /// carries the same id stays idempotent. `patch`/`replace`/`update`/`upsert`
    /// never touch the id. Backends that mint their own id (e.g. the in-memory
    /// store, a SQL column with a `DEFAULT`) ignore the filled value and need no
    /// generator.
    pub fn with_generated_id(self, generator: IdGenerator) -> Self {
        self.with_hook(Hook::BeforeInsert(
            Phase::Populate,
            id_fill_hook::<T>(generator),
        ))
    }

    /// Mutable form of [`Self::with_generated_id`].
    pub fn add_generated_id(&mut self, generator: IdGenerator) {
        self.add_hook(Hook::BeforeInsert(
            Phase::Populate,
            id_fill_hook::<T>(generator),
        ));
    }
}

/// Build the before-insert hook that fills an absent/null id from `generator`.
/// The id column name is resolved from the table at fire time (falling back to
/// `"id"`), so the column need not exist when the generator is registered.
fn id_fill_hook<T: TableSource>(generator: IdGenerator) -> BeforeFn<T>
where
    T::Value: InvariantValue + From<String>,
{
    Arc::new(
        move |rec: &mut Record<T::Value>,
              table: &Table<T, EmptyEntity>|
              -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            let id_field = table
                .id_field()
                .map(|c| c.name().to_string())
                .unwrap_or_else(|| "id".to_string());
            let generator = generator.clone();
            Box::pin(async move {
                let absent_or_null = match rec.get(id_field.as_str()) {
                    None => true,
                    Some(v) => v.is_null(),
                };
                if absent_or_null {
                    rec.insert(id_field, T::Value::from(generator.generate()));
                }
                Ok(())
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use serde_json::json;
    use vantage_dataset::prelude::{InsertableValueSet, ReadableValueSet, WritableValueSet};

    type MockTable = Table<MockTableSource, EmptyEntity>;

    #[tokio::test]
    async fn fills_missing_id_on_insert() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = MockTable::new("t", src).with_generated_id(IdGenerator::UuidV7);

        let id = table
            .insert_return_id_value(&Record::from(json!({"name": "a"})))
            .await
            .unwrap();
        // A v7 uuid is a non-empty, hyphenated string and is now the row's key.
        assert!(id.contains('-'));
        assert_eq!(
            table.get_value(id).await.unwrap().unwrap()["name"],
            json!("a")
        );
    }

    #[tokio::test]
    async fn keeps_caller_supplied_id() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = MockTable::new("t", src).with_generated_id(IdGenerator::UuidV7);

        // A present id is preserved — the generator never overwrites it.
        let id = table
            .insert_return_id_value(&Record::from(json!({"id": "fixed-1", "name": "a"})))
            .await
            .unwrap();
        assert_eq!(id, "fixed-1");
    }

    #[tokio::test]
    async fn fills_when_id_is_null() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table = MockTable::new("t", src).with_generated_id(IdGenerator::UuidV4);

        let id = table
            .insert_return_id_value(&Record::from(json!({"id": null, "name": "a"})))
            .await
            .unwrap();
        assert!(!id.is_empty() && id != "null");
    }

    #[tokio::test]
    async fn custom_generator_is_used() {
        let src = MockTableSource::new().with_data("t", vec![]).await;
        let table =
            MockTable::new("t", src).with_generated_id(IdGenerator::custom(|| "made-up".into()));

        let id = table
            .insert_return_id_value(&Record::from(json!({"name": "a"})))
            .await
            .unwrap();
        assert_eq!(id, "made-up");
    }

    #[tokio::test]
    async fn patch_does_not_touch_id() {
        let src = MockTableSource::new()
            .with_data("t", vec![json!({"id": "1", "name": "a"})])
            .await;
        let table =
            MockTable::new("t", src).with_generated_id(IdGenerator::custom(|| "made-up".into()));

        // patch fires before_update, not before_insert — the generator is a
        // before_insert hook, so the id is left alone.
        table
            .patch_value("1", &Record::from(json!({"name": "b"})))
            .await
            .unwrap();
        let row = table.get_value("1".to_string()).await.unwrap().unwrap();
        assert_eq!(row["id"], json!("1"));
        assert_eq!(row["name"], json!("b"));
    }
}
