//! Grouping, with the per-group reduction supplied by the caller.

use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{Column, flags};

use crate::aggregation::{Aggregation, DerivedRows, Rows};
use crate::cmp;

/// Reduces one group's rows to one derived row.
///
/// Implement this for anything beyond a one-liner; for the common case reach
/// for [`Reduce`], which wraps a closure.
pub trait GroupReducer: Send + Sync + 'static {
    /// Columns this reducer emits, *excluding* the group key — [`GroupBy`]
    /// prepends that. These become real columns on the derived table.
    fn columns(&self) -> Vec<Column>;

    /// Reduce one group. `rows` is never empty, and arrives in the source's
    /// order.
    fn reduce(&self, key: &CborValue, rows: &[&Record<CborValue>]) -> Record<CborValue>;
}

/// A [`GroupReducer`] built from a closure.
///
/// ```ignore
/// Reduce::new(
///     vec![Column::new("orders", "i64"), Column::new("revenue", "f64")],
///     |_key, rows| {
///         let mut out = Record::new();
///         out.insert("orders".into(), (rows.len() as i64).into());
///         out
///     },
/// )
/// ```
pub struct Reduce<F> {
    columns: Vec<Column>,
    reduce: F,
}

impl<F> Reduce<F>
where
    F: Fn(&CborValue, &[&Record<CborValue>]) -> Record<CborValue> + Send + Sync + 'static,
{
    pub fn new(columns: Vec<Column>, reduce: F) -> Self {
        Self { columns, reduce }
    }
}

impl<F> GroupReducer for Reduce<F>
where
    F: Fn(&CborValue, &[&Record<CborValue>]) -> Record<CborValue> + Send + Sync + 'static,
{
    fn columns(&self) -> Vec<Column> {
        self.columns.clone()
    }

    fn reduce(&self, key: &CborValue, rows: &[&Record<CborValue>]) -> Record<CborValue> {
        (self.reduce)(key, rows)
    }
}

/// Group rows by a column, then reduce each group.
///
/// Rows whose key column is absent are skipped — a missing key is not a group.
/// Output is ordered by key, so the same input always produces the same table.
pub struct GroupBy<R> {
    key_column: String,
    reducer: R,
}

impl<R: GroupReducer> GroupBy<R> {
    /// Group by `key_column`, which may be a dotted path into a nested value.
    pub fn column(key_column: impl Into<String>, reducer: R) -> Self {
        Self {
            key_column: key_column.into(),
            reducer,
        }
    }
}

impl<R: GroupReducer> Aggregation for GroupBy<R> {
    type Output = DerivedRows;

    fn compute(&self, rows: &Rows) -> DerivedRows {
        // Grouped in first-seen order, then sorted by key below. A hash map
        // would make the output order depend on hashing, which would look like
        // a change on every recomputation.
        let mut groups: Vec<(CborValue, Vec<&Record<CborValue>>)> = Vec::new();
        for record in rows.values() {
            let Some(key) = cmp::column(record, &self.key_column) else {
                continue;
            };
            match groups.iter_mut().find(|(existing, _)| existing == key) {
                Some((_, members)) => members.push(record),
                None => groups.push((key.clone(), vec![record])),
            }
        }

        groups.sort_by(|(a, _), (b, _)| cmp::compare(a, b));

        let derived = groups
            .into_iter()
            .map(|(key, members)| {
                let mut record = self.reducer.reduce(&key, &members);
                // The key is part of the derived row, and the reducer must not
                // be able to overwrite the column the id is read from.
                record.insert(self.key_column.clone(), key.clone());
                (key_id(&key), record)
            })
            .collect();

        let mut columns = vec![Column::new(self.key_column.clone(), "String").with_flag(flags::ID)];
        columns.extend(self.reducer.columns());

        DerivedRows::new(derived, columns)
    }
}

/// Stable row id for a group key.
fn key_id(key: &CborValue) -> String {
    match key {
        CborValue::Text(text) => text.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Float(f) => f.to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Null => "null".to_string(),
        other => format!("{other:?}"),
    }
}
