//! [`TableGen`]: one table's generation plan — columns, row count, row
//! content settings, and links to the other tables it references.

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::ExtraFields;
use crate::relational::{Reference, max_relational_rows, relational_rows};
use crate::value_gen::ValueGen;
use crate::{FakerColumn, FanOut};

/// A declared reference: `column` holds ids of `target`, resolved to a
/// [`Reference`] once `target`'s row count is known.
#[derive(Clone, Debug)]
pub(super) struct DeclaredRef {
    pub column: String,
    pub target: String,
}

/// One table to generate: its columns, row count, and any reference
/// columns pointing at other tables in the same [`DatasetGen`](super::DatasetGen).
#[derive(Clone, Debug, Default)]
pub struct TableGen {
    pub(super) name: String,
    pub(super) id_column: String,
    pub(super) columns: Vec<FakerColumn>,
    pub(super) count: usize,
    pub(super) refs: Vec<DeclaredRef>,
    pub(super) fan_out: Option<FanOut>,
    pub(super) indexed: Vec<String>,
    weirdness: f64,
    extra_fields: Option<ExtraFields>,
}

impl TableGen {
    /// A table named `name`, with the default `id` id column and no rows.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id_column: "id".into(),
            ..Self::default()
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Store rows under this id column instead of `"id"`.
    pub fn id_column(mut self, id_column: impl Into<String>) -> Self {
        self.id_column = id_column.into();
        self
    }

    /// Add one column.
    pub fn column(mut self, column: FakerColumn) -> Self {
        self.columns.push(column);
        self
    }

    /// Add several columns.
    pub fn columns(mut self, columns: impl IntoIterator<Item = FakerColumn>) -> Self {
        self.columns.extend(columns);
        self
    }

    /// Rows to generate. Ignored when [`fan_out`](Self::fan_out) is set on a
    /// [`reference`](Self::reference) column — the fan-out decides the count.
    pub fn count(mut self, count: usize) -> Self {
        self.count = count;
        self
    }

    /// Declare `column` as holding ids of `target_table`, generated earlier
    /// in the same [`DatasetGen`](super::DatasetGen).
    pub fn reference(mut self, column: impl Into<String>, target_table: impl Into<String>) -> Self {
        self.refs.push(DeclaredRef {
            column: column.into(),
            target: target_table.into(),
        });
        self
    }

    /// Vary children-per-parent on a [`reference`](Self::reference) column
    /// instead of a flat [`count`](Self::count).
    pub fn fan_out(mut self, fan_out: FanOut) -> Self {
        self.fan_out = Some(fan_out);
        self
    }

    /// Columns to hash-index in the store.
    pub fn indexed(mut self, columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.indexed = columns.into_iter().map(Into::into).collect();
        self
    }

    /// Fraction (`0..=1`) of generated string cells drawn from the anomaly
    /// pool — see [`ValueGen::with_weirdness`]. Default 0.
    pub fn weirdness(mut self, weirdness: f64) -> Self {
        self.weirdness = weirdness;
        self
    }

    /// Ride [`ExtraFields`] filler on every generated row.
    pub fn extra_fields(mut self, extra: ExtraFields) -> Self {
        self.extra_fields = Some(extra);
        self
    }

    /// Most rows [`rows`](Self::rows) generates with `refs` resolved.
    pub(super) fn max_rows(&self, refs: &[Reference]) -> usize {
        max_relational_rows(self.count, refs, self.fan_out.as_ref())
    }

    /// This table's rows, drawn from `seed` with `refs` resolved.
    pub(super) fn rows(
        &self,
        seed: Option<u64>,
        refs: &[Reference],
    ) -> Vec<(String, Record<CborValue>)> {
        let values = ValueGen::from_seed(seed).with_weirdness(self.weirdness);
        let mut rows = relational_rows(
            &values,
            &self.columns,
            &self.id_column,
            self.count,
            refs,
            self.fan_out.as_ref(),
        );
        if let Some(extra) = &self.extra_fields {
            for (id, record) in &mut rows {
                extra.apply(id, record);
            }
        }
        rows
    }
}
