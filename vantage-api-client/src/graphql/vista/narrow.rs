//! Narrowing a `GraphqlApiTableShell`: conditions, quicksearch and order.
//! Each clause is checked before it is pushed, so a refused one leaves the
//! shell's query as it was.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::traits::table_source::TableSource as _;
use vantage_vista::{Capability, Condition, FilterOp, SortDirection, operand_text, unsupported};

use crate::graphql::impls::search::contains_pattern;
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::source::GraphqlApiTableShell;

impl GraphqlApiTableShell {
    /// Equality always; any other operator only where the dialect spells it
    /// (Hasura, with filter push-down on). Elsewhere it is refused, and the
    /// consumer evaluates it over the rows it reads.
    pub(crate) fn apply_condition(&mut self, condition: &Condition) -> Result<()> {
        let op = condition.op;
        if op != FilterOp::Eq && !self.capabilities.has(Capability::FilterOps) {
            return unsupported(Capability::FilterOps);
        }
        let column = Column::<AnyGraphqlType>::new(&condition.column);
        let scalar = || AnyGraphqlType::from(condition.value.clone());
        let expression = match op {
            FilterOp::Eq => column.eq(scalar()),
            FilterOp::Ne => column.ne(scalar()),
            FilterOp::Gt => column.gt(scalar()),
            FilterOp::Gte => column.gte(scalar()),
            FilterOp::Lt => column.lt(scalar()),
            FilterOp::Lte => column.lte(scalar()),
            FilterOp::InSet | FilterOp::NotInSet => {
                let items: Vec<AnyGraphqlType> = match &condition.value {
                    CborValue::Array(items) => {
                        items.iter().cloned().map(AnyGraphqlType::from).collect()
                    }
                    single => vec![AnyGraphqlType::from(single.clone())],
                };
                if op == FilterOp::InSet {
                    column.in_(items)
                } else {
                    column.not_in(items)
                }
            }
            // Hasura spells "contains, any case" as `_ilike '%…%'`.
            FilterOp::Like => column.ilike(contains_pattern(&operand_text(&condition.value))),
            _ => return unsupported(Capability::FilterOps),
        };
        self.table.add_condition(expression);
        Ok(())
    }

    /// Replace the quicksearch condition; `None` drops it.
    pub(crate) fn apply_search(&mut self, text: Option<&str>) {
        if let Some(handle) = self.search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        if let Some(text) = text {
            let api = self.table.data_source().clone();
            let condition = api.search_table_condition(&self.table, text);
            self.search_handle = Some(self.table.temp_add_condition(condition));
        }
    }

    /// Replace the order; `None` drops it.
    pub(crate) fn apply_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        let Some((field, dir)) = order else {
            self.table.clear_orders();
            return Ok(());
        };
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for order", field = field));
        }
        // Hasura orders a nested field with a nested `order_by` object,
        // which the renderer doesn't produce; `run.id: asc` would be
        // rejected by the server.
        if field.contains('.') {
            return Err(
                error!("GraphQL cannot order by a dotted column", field = field).mark_unsupported(),
            );
        }
        self.table.clear_orders();
        // The table's order list is typed as conditions, and the renderer
        // reads the column name back out of the `Field` variant — so an
        // eq-condition on the column is how an order carries its field.
        // The value is never rendered.
        let direction = match dir {
            SortDirection::Ascending => TableSortDirection::Ascending,
            SortDirection::Descending => TableSortDirection::Descending,
        };
        self.table.add_order(OrderBy {
            expression: Column::<AnyGraphqlType>::new(field).eq(field),
            direction,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use vantage_table::table::Table;
    use vantage_types::EmptyEntity;
    use vantage_vista::{TableShell, VistaCapabilities, VistaMetadata};

    use super::*;
    use crate::graphql::api::GraphqlApi;

    fn shell(capabilities: VistaCapabilities) -> GraphqlApiTableShell {
        let api = GraphqlApi::new("https://api.test/graphql");
        let table = Table::<GraphqlApi, EmptyEntity>::new("runs", api)
            .with_id_column("id")
            .with_column_of::<String>("run.id");
        GraphqlApiTableShell::new(table, capabilities, VistaMetadata::new())
    }

    /// The shell refuses a dotted order on its own, not only through the
    /// Vista's `Orderable` check, so a caller reaching the shell directly
    /// can't render an `order_by` the server rejects.
    #[test]
    fn shell_refuses_ordering_a_dotted_column() {
        let mut shell = shell(VistaCapabilities::new().with(Capability::Order));
        let err = shell
            .set_order(Some(("run.id", SortDirection::Ascending)))
            .unwrap_err();
        assert!(err.is_unsupported(), "{err}");
        assert!(shell.table.orders().next().is_none());
    }

    /// Without `FilterOps` (a Generic dialect) an operator is refused and
    /// leaves no condition behind.
    #[test]
    fn shell_refuses_operators_without_filter_ops() {
        let mut shell = shell(VistaCapabilities::new());
        let gt = Condition::new("id", FilterOp::Gt, "5");
        assert!(shell.add_condition(&gt).unwrap_err().is_unsupported());
        assert_eq!(shell.table.conditions().count(), 0);
    }
}
