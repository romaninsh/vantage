//! Computed columns at the `Vista` boundary: filled on every read, dropped
//! from every write, refused as a filter, order or has-many join key. A
//! column becomes computed through
//! [`Column::with_expression`](crate::Column::with_expression).

use std::borrow::Cow;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Context, Result, error};
use vantage_types::Record;

use crate::{column::Column, reference::ReferenceKind, vista::Vista};

type Rec = Record<CborValue>;

/// Fill the computed `columns` into `row`, in order, so each sees the values
/// of those before it. Stored columns are skipped.
pub(crate) fn fill<'a>(columns: impl IntoIterator<Item = &'a Column>, row: &mut Rec) -> Result<()> {
    for column in columns {
        if let Some(value) = column.compute(row) {
            let value = value
                .with_context(|| error!("Computed column failed", column = column.name.clone()))?;
            row.insert(column.name.clone(), value);
        }
    }
    Ok(())
}

impl Vista {
    /// The computed columns, in declaration order.
    pub(crate) fn computed_columns(&self) -> Vec<Column> {
        self.source
            .columns()
            .values()
            .filter(|c| c.is_computed())
            .cloned()
            .collect()
    }

    pub(crate) fn has_computed(&self) -> bool {
        self.source.columns().values().any(Column::is_computed)
    }

    pub(crate) fn fill_computed(&self, row: &mut Rec) -> Result<()> {
        fill(self.source.columns().values(), row)
    }

    pub(crate) fn fill_computed_rows<'a>(
        &self,
        rows: impl IntoIterator<Item = &'a mut Rec>,
    ) -> Result<()> {
        if !self.has_computed() {
            return Ok(());
        }
        for row in rows {
            self.fill_computed(row)?;
        }
        Ok(())
    }

    /// `record` without its computed columns: the backend has nowhere to
    /// store them.
    pub(crate) fn without_computed<'a>(&self, record: &'a Rec) -> Cow<'a, Rec> {
        let mut present = self
            .source
            .columns()
            .values()
            .filter(|c| c.is_computed() && record.contains_key(&c.name))
            .peekable();
        if present.peek().is_none() {
            return Cow::Borrowed(record);
        }
        let mut owned = record.clone();
        for column in present {
            owned.shift_remove(&column.name);
        }
        Cow::Owned(owned)
    }

    /// [`without_computed`](Self::without_computed) over a batch.
    pub(crate) fn without_computed_all<'a>(
        &self,
        records: &'a IndexMap<String, Rec>,
    ) -> Cow<'a, IndexMap<String, Rec>> {
        if !self.has_computed() {
            return Cow::Borrowed(records);
        }
        Cow::Owned(
            records
                .iter()
                .map(|(id, r)| (id.clone(), self.without_computed(r).into_owned()))
                .collect(),
        )
    }

    /// Refuse `operation` on `column` when it is computed: the backend holds
    /// no values to filter, order or aggregate by.
    pub(crate) fn refuse_computed(&self, column: &str, operation: &str) -> Result<()> {
        match self.get_column(column) {
            Some(c) if c.is_computed() => Err(error!(
                "Computed column can't be used to filter, order or aggregate; the backend doesn't hold its values",
                column = column,
                operation = operation
            )
            .mark_unsupported()),
            _ => Ok(()),
        }
    }

    /// A has-many traversal narrows `target` by its foreign key on the
    /// backend, so that key can't be a computed column of the target.
    pub(crate) fn check_has_many_key(&self, relation: &str, target: &Vista) -> Result<()> {
        let Some(reference) = self.get_reference(relation) else {
            return Ok(());
        };
        if reference.kind == ReferenceKind::HasMany
            && target
                .get_column(&reference.foreign_key)
                .is_some_and(Column::is_computed)
        {
            return Err(error!(
                "Has-many relation can't join on a computed column of its target",
                relation = relation,
                column = reference.foreign_key.clone()
            )
            .mark_unsupported());
        }
        Ok(())
    }

    /// A nested insert links rows by writing a foreign key: into this row for
    /// a has-one, into each child for a has-many. Neither can be a computed
    /// column, which the backend has nowhere to store.
    pub(crate) fn check_insert_link(&self, relation: &str) -> Result<()> {
        let Some(reference) = self.get_reference(relation) else {
            return Ok(());
        };
        match reference.kind {
            ReferenceKind::HasOne => {
                if self
                    .get_column(&reference.foreign_key)
                    .is_some_and(Column::is_computed)
                {
                    return Err(error!(
                        "Nested insert can't link through a computed foreign key",
                        relation = relation,
                        column = reference.foreign_key.clone()
                    )
                    .mark_unsupported());
                }
                Ok(())
            }
            ReferenceKind::HasMany => {
                self.check_has_many_key(relation, &self.get_ref_target(relation)?)
            }
        }
    }
}
