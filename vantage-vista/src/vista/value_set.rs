//! The vantage-dataset value-set traits over a [`Vista`]. Reads fill the
//! computed columns; writes drop them and stay inside a narrowed set.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::{InsertableValueSet, ReadableValueSet, ValueSet, WritableValueSet};
use vantage_types::Record;

use super::Vista;
use crate::capabilities::Capability;

type Rec = Record<CborValue>;

impl ValueSet for Vista {
    type Id = String;
    type Value = CborValue;
}

#[async_trait]
impl ReadableValueSet for Vista {
    async fn list_values(&self) -> Result<IndexMap<String, Rec>> {
        let mut rows = self.shell().list().await?;
        self.fill_computed_rows(rows.values_mut())?;
        Ok(rows)
    }

    async fn get_value(&self, id: impl Into<String> + Send) -> Result<Option<Rec>> {
        let mut row = self.shell().get(&id.into()).await?;
        self.fill_computed_rows(row.as_mut())?;
        Ok(row)
    }

    /// The first row of the set: the leading window `fetch_window(0, 1)`,
    /// which every shell that lists answers.
    async fn get_some_value(&self) -> Result<Option<(String, Rec)>> {
        Ok(self.fetch_window(0, 1).await?.rows.into_iter().next())
    }
}

#[async_trait]
impl WritableValueSet for Vista {
    async fn insert_value(&self, id: impl Into<String> + Send, record: &Rec) -> Result<Rec> {
        self.require_write(Capability::Insert, "insert")?;
        self.insert_nested_value(&id.into(), record).await
    }

    async fn replace_value(&self, id: impl Into<String> + Send, record: &Rec) -> Result<Rec> {
        self.require_write(Capability::Update, "replace")?;
        let record = self.without_computed(record);
        self.shell().replace(&id.into(), &record).await
    }

    async fn patch_value(&self, id: impl Into<String> + Send, partial: &Rec) -> Result<Rec> {
        self.require_write(Capability::Update, "patch")?;
        let partial = self.without_computed(partial);
        self.shell().patch(&id.into(), &partial).await
    }

    async fn delete(&self, id: impl Into<String> + Send) -> Result<()> {
        self.require_write(Capability::Delete, "delete")?;
        self.shell().delete(&id.into()).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.require_write(Capability::Delete, "delete_all")?;
        self.shell().delete_all().await
    }
}

#[async_trait]
impl InsertableValueSet for Vista {
    async fn insert_return_id_value(&self, record: &Rec) -> Result<String> {
        self.require_write(Capability::Insert, "insert")?;
        self.insert_nested_return_id(record).await
    }
}

impl Vista {
    /// Insert row `id`, or replace it if it exists, in one backend operation.
    ///
    /// Inherent rather than on [`WritableValueSet`]: that trait is shared
    /// with `ImTable` and `Table<T, E>`, and `upsert` is a `Vista`-only verb.
    pub async fn upsert_value(&self, id: impl Into<String> + Send, record: &Rec) -> Result<Rec> {
        self.require_write(Capability::Update, "upsert")?;
        let record = self.without_computed(record);
        self.shell().upsert(&id.into(), &record).await
    }
}
