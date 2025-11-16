use vantage_core::Entity;
use vantage_expressions::{SelectSource, Selectable};

use crate::{table::Table, traits::table_source::TableSource};

impl<T, E> Table<T, E>
where
    T: SelectSource<T::Value> + TableSource,
    T::Select: Selectable<T::Value>,
    E: Entity,
{
    /// Create a select query with table configuration applied
    pub fn select(&self) -> T::Select {
        let mut select = self.data_source.select();

        // Set the table as source
        select.set_source(self.table_name(), None);

        // Add all columns as fields
        for column in self.columns.values() {
            match column.alias() {
                Some(alias) => select.add_expression(
                    self.data_source.expr(column.name(), vec![]),
                    Some(alias.to_string()),
                ),
                None => select.add_field(column.name()),
            }
        }

        // Add all conditions
        for condition in self.conditions.values() {
            select.add_where_condition(condition.clone());
        }

        // Add all order clauses
        for (expr, direction) in self.order_by.values() {
            let ascending = matches!(direction, crate::sorting::SortDirection::Ascending);
            select.add_order_by(expr.clone(), ascending);
        }

        // Apply pagination
        if let Some(pagination) = &self.pagination {
            let (limit, skip) = pagination.get_limit_and_skip();
            select.set_limit(limit, skip);
        }

        select
    }
}
