//! The vantage-dataset value-set traits over a [`Vista`]. Reads fill the
//! computed columns; writes drop them and stay inside a narrowed set.

use std::borrow::Cow;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::{InsertableValueSet, ReadableValueSet, ValueSet, WritableValueSet};
use vantage_types::Record;

use super::Vista;
use crate::{
    capabilities::Capability,
    column::ColumnFlag,
    denials::{forbidden, forbidden_column},
};

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
        let id = id.into();
        let record = self.whole_row(&id, record).await?;
        self.shell().replace(&id, &record).await
    }

    async fn patch_value(&self, id: impl Into<String> + Send, partial: &Rec) -> Result<Rec> {
        self.require_write(Capability::Update, "patch")?;
        self.refuse_read_only(partial)?;
        let partial = self.without_computed(partial);
        self.shell().patch(&id.into(), &partial).await
    }

    async fn delete(&self, id: impl Into<String> + Send) -> Result<()> {
        self.require_write(Capability::Delete, "delete")?;
        self.shell().delete(&id.into()).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.require_write(Capability::DeleteAll, "delete_all")?;
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
    ///
    /// It may create the row, so a rule denying `Insert` refuses it too.
    pub async fn upsert_value(&self, id: impl Into<String> + Send, record: &Rec) -> Result<Rec> {
        self.require_write(Capability::Update, "upsert")?;
        if let Some(rule) = self.denied(Capability::Insert) {
            return forbidden(Capability::Insert, rule, self.name());
        }
        let id = id.into();
        let record = self.whole_row(&id, record).await?;
        self.shell().upsert(&id, &record).await
    }

    /// What a whole-row write of `id` stores: `record` without its computed
    /// columns, and with each column a rule made read-only holding its stored
    /// value, so the write neither sets nor erases it. A read-only column
    /// that is calculated, or missing from the stored row, is dropped. When
    /// a rule denies `Insert`, a row that isn't stored yet is refused.
    async fn whole_row<'a>(&self, id: &str, record: &'a Rec) -> Result<Cow<'a, Rec>> {
        let record = self.without_computed(record);
        let denials = self.denials();
        let kept: Vec<&str> = denials
            .columns()
            .map(|(column, _)| column)
            .filter(|name| {
                !self
                    .column(name)
                    .is_some_and(|c| c.has_flag(ColumnFlag::Calculated))
            })
            .collect();
        let insert_rule = denials.rule_for(Capability::Insert);
        let stored = if kept.is_empty() && insert_rule.is_none() {
            None
        } else {
            self.shell().get(id).await?
        };
        if let (Some(rule), None) = (insert_rule, &stored) {
            return forbidden(Capability::Insert, rule, self.name());
        }
        if kept.is_empty() && !record.keys().any(|c| denials.column_rule(c).is_some()) {
            return Ok(record);
        }
        let mut owned = record.into_owned();
        owned.retain(|c, _| denials.column_rule(c).is_none());
        if let Some(stored) = &stored {
            for column in kept {
                if let Some(value) = stored.get(column) {
                    owned.insert(column.to_string(), value.clone());
                }
            }
        }
        Ok(Cow::Owned(owned))
    }

    /// A patch names the columns it sets, so a read-only one is refused.
    fn refuse_read_only(&self, partial: &Rec) -> Result<()> {
        match partial
            .keys()
            .find_map(|c| self.denied_column(c).map(|rule| (c, rule)))
        {
            Some((column, rule)) => forbidden_column(column, rule, self.name()),
            None => Ok(()),
        }
    }
}
