//! The fields a table's returned rows carry, and the in-memory filter that
//! relies on them.

use indexmap::IndexMap;
use vantage_types::{Entity, Record};

use crate::{
    column::flags::ColumnFlag,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// The declared columns a returned row carries, in declaration order:
    /// every column except those flagged
    /// [`RequestParam`](ColumnFlag::RequestParam), whose values go to the
    /// backend rather than come back from it. A `Hidden` column is still a
    /// row field.
    pub fn row_fields(&self) -> Vec<String> {
        self.columns()
            .iter()
            .filter(|(_, column)| !column.flags().contains(&ColumnFlag::RequestParam))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

/// Keep the rows that match every `(field, wanted)` equality filter, for a
/// backend that filters fetched rows in memory.
///
/// A row holding `field` passes when `eq(value, wanted)`. A row without it
/// fails when `field` is one of `row_fields` (see [`Table::row_fields`]):
/// rows were promised to carry it. Any other field (a `RequestParam` column,
/// a URL placeholder, a parameter the table doesn't declare) went to the
/// backend, which applied it without returning it, so its absence passes.
pub fn retain_matching<V, W: ?Sized>(
    rows: &mut IndexMap<String, Record<V>>,
    filters: &[(&str, &W)],
    row_fields: &[String],
    eq: impl Fn(&V, &W) -> bool,
) {
    rows.retain(|_, row| {
        filters.iter().all(|(field, wanted)| match row.get(*field) {
            Some(value) => eq(value, wanted),
            None => !row_fields.iter().any(|f| f == field),
        })
    });
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use vantage_types::{EmptyEntity, Record};

    use super::retain_matching;
    use crate::column::flags::ColumnFlag;
    use crate::mocks::{mock_column::MockColumn, mock_table_source::MockTableSource};
    use crate::table::Table;

    #[test]
    fn a_missing_field_fails_only_when_rows_carry_it() {
        let row = |pairs: &[(&str, i64)]| -> Record<i64> {
            pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
        };
        let mut rows: IndexMap<String, Record<i64>> = [
            ("a".to_string(), row(&[("size", 1)])),
            ("b".to_string(), row(&[("size", 2)])),
            ("c".to_string(), row(&[])),
        ]
        .into_iter()
        .collect();
        let row_fields = vec!["size".to_string()];

        // `prefix` is no row field: its absence passes.
        retain_matching(&mut rows, &[("prefix", &9)], &row_fields, |a, b| a == b);
        assert_eq!(rows.len(), 3);

        // `size` is: the row without it fails, as does the mismatch.
        retain_matching(&mut rows, &[("size", &1)], &row_fields, |a, b| a == b);
        assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"]);
    }

    #[test]
    fn row_fields_leave_out_request_params_but_keep_hidden_columns() {
        let table = Table::<MockTableSource, EmptyEntity>::new("buckets", MockTableSource::new())
            .with_column(MockColumn::<String>::new("region").with_flag(ColumnFlag::RequestParam))
            .with_column(MockColumn::<String>::new("name"))
            .with_column(MockColumn::<String>::new("owner").with_flag(ColumnFlag::Hidden));
        assert_eq!(table.row_fields(), ["name", "owner"]);
    }
}
