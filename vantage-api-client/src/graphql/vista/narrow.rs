//! Narrowing a `GraphqlApiTableShell`: conditions, quicksearch and order.
//! Each clause is checked before it is pushed, so a refused one leaves the
//! shell's query as it was.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::traits::table_source::TableSource as _;
use vantage_vista::{
    Capability, Condition, FilterOp, SortDirection, condition_column_error, operand_text,
    unsupported,
};

use crate::graphql::impls::get_by_id::{id_for_column, is_integer_column};
use crate::graphql::impls::search::contains_pattern;
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::types::AnyGraphqlType;
use crate::graphql::vista::source::GraphqlApiTableShell;

impl GraphqlApiTableShell {
    /// Equality always; any other operator only where the dialect spells it
    /// (Hasura, with filter push-down on). Elsewhere it is refused, and the
    /// consumer evaluates it over the rows it reads. Each operand is typed by
    /// its column ([`operand`](Self::operand)). With filter push-down
    /// on, a dotted path that isn't a declared column (`id.part`) can't be
    /// sent as a filter key: under a declared column it is refused with
    /// [`condition_column_error`], for the caller to
    /// evaluate, and under an unknown one it is a normal error. Filtering
    /// locally, the path is walked in each row.
    pub(crate) fn apply_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if field.contains('.')
            && self.table.data_source().can_filter()
            && !self.table.columns().contains_key(field)
        {
            return Err(condition_column_error(&self.metadata, field));
        }
        let op = condition.op;
        if op != FilterOp::Eq && !self.capabilities.has(Capability::FilterOps) {
            return unsupported(Capability::FilterOps);
        }
        let column = Column::<AnyGraphqlType>::new(&condition.column);
        let scalar = || self.operand(field, &condition.value);
        let expression = match op {
            FilterOp::Eq => column.eq(scalar()?),
            FilterOp::Ne => column.ne(scalar()?),
            FilterOp::Gt => column.gt(scalar()?),
            FilterOp::Gte => column.gte(scalar()?),
            FilterOp::Lt => column.lt(scalar()?),
            FilterOp::Lte => column.lte(scalar()?),
            FilterOp::InSet | FilterOp::NotInSet => {
                let items = match &condition.value {
                    CborValue::Array(items) => items
                        .iter()
                        .map(|item| self.operand(field, item))
                        .collect::<Result<Vec<_>>>()?,
                    single => vec![self.operand(field, single)?],
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

    /// `value` as the scalar the server compares `field` with. Text on an
    /// integer-typed column is sent as an integer: rows and scripts carry
    /// ids as text, and the server rejects `_eq: "5"` against an `Int`.
    fn operand(&self, field: &str, value: &CborValue) -> Result<AnyGraphqlType> {
        let column = self.table.columns().get(field);
        match value {
            CborValue::Text(text) if is_integer_column(column) => {
                Ok(AnyGraphqlType::untyped(id_for_column(column, text, field)?))
            }
            other => Ok(AnyGraphqlType::from(other.clone())),
        }
    }

    /// Replace the quicksearch condition; `None` drops it. The old handle is
    /// cleared only once its condition is removed.
    pub(crate) fn apply_search(&mut self, text: Option<&str>) -> Result<()> {
        if let Some(handle) = self.search_handle {
            self.table.temp_remove_condition(handle)?;
            self.search_handle = None;
        }
        if let Some(text) = text {
            let api = self.table.data_source().clone();
            let condition = api.search_table_condition(&self.table, text);
            self.search_handle = Some(self.table.temp_add_condition(condition));
        }
        Ok(())
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
        shell_over(GraphqlApi::new("https://api.test/graphql"), capabilities)
    }

    fn shell_over(api: GraphqlApi, capabilities: VistaCapabilities) -> GraphqlApiTableShell {
        let table = Table::<GraphqlApi, EmptyEntity>::new("runs", api)
            .with_id_column("id")
            .with_column_of::<String>("run.id");
        GraphqlApiTableShell::new(table, capabilities, VistaMetadata::new())
    }

    /// An undeclared dotted path is refused when it would be pushed down as
    /// a filter key, and accepted for local evaluation when filtering is off.
    #[test]
    fn an_undeclared_dotted_path_is_refused_only_for_push_down() {
        let probe = Condition::new("id.part", FilterOp::Eq, "x");
        let mut pushed = shell(VistaCapabilities::new());
        let err = pushed.add_condition(&probe).unwrap_err();
        assert!(err.is_unimplemented(), "{err:?}");
        assert_eq!(pushed.table.conditions().count(), 0);

        // Under a column the schema doesn't declare, the path is unknown.
        let declared = VistaMetadata::new().with_column(vantage_vista::Column::new("id", "String"));
        let mut known = GraphqlApiTableShell::new(
            Table::<GraphqlApi, EmptyEntity>::new(
                "runs",
                GraphqlApi::new("https://api.test/graphql"),
            ),
            VistaCapabilities::new(),
            declared,
        );
        let unknown = Condition::new("colour.hue", FilterOp::Eq, "x");
        let err = known.add_condition(&unknown).unwrap_err();
        assert!(!err.is_unimplemented() && !err.is_unsupported(), "{err:?}");

        let local_api = GraphqlApi::builder("https://api.test/graphql")
            .supports(crate::graphql::api::Supports::default().with_filter(false))
            .build();
        let mut local = shell_over(local_api, VistaCapabilities::new());
        local.add_condition(&probe).unwrap();
        assert_eq!(local.table.conditions().count(), 1);
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
