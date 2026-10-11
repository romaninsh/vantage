//! Narrowing the wrapped table: conditions, native conditions, quicksearch
//! and order. Each clause is checked before it is pushed, so a refused one
//! leaves the shell's query as it was.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::Expressive;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};
use vantage_vista::{
    Capability, Condition, FilterOp, NativeCondition, SortDirection, operand_text, unsupported,
};

use super::SurrealTableShell;
use crate::Expr;
use crate::identifier::Identifier;
use crate::operation::SurrealOperation;
use crate::surreal_expr;
use crate::thing::Thing;
use crate::types::{AnySurrealType, SurrealType};

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    /// The left-hand side of a condition on `field`.
    ///
    /// A usual column is its identifier. A server-side `expr:` column is
    /// projected as `(<expr>) AS <name>`, but SurrealDB evaluates WHERE
    /// before the projection, so the alias is not a field there and a
    /// condition on it matches nothing. Such a column is therefore filtered
    /// on its (parenthesised) expression.
    ///
    /// `None` when the table has no such column.
    fn condition_subject(&self, field: &str) -> Option<Expr> {
        if self.table.is_calculated_column(field)
            && let Some(expression) = self.table.get_column_expr(field)
        {
            return Some(surreal_expr!("({})", (expression)));
        }
        self.table.columns().get(field).map(|column| column.expr())
    }

    pub(super) fn apply_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        let subject = self
            .condition_subject(field)
            .ok_or_else(|| error!("Unknown column for condition", field = field))?;
        let value = &condition.value;
        // Id-column comparisons coerce a string value into a Thing —
        // SurrealDB compares record ids by type, so a quoted string
        // (`id = "MG78-N25S"` or `id = "tag:MG78-N25S"`) never matches.
        // `parse_id` accepts both the bare-key and `table:key` forms. This is
        // what lets a cross-persistence relation narrow a surreal table from
        // a plain string held by another backend's row.
        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let operand = || match value {
            CborValue::Text(s) if field == id_field => {
                AnySurrealType::from(self.parse_id(s).to_cbor())
            }
            _ => AnySurrealType::from(value.clone()),
        };
        let expr = match condition.op {
            FilterOp::Eq => subject.eq(operand()),
            FilterOp::Ne => subject.ne(operand()),
            FilterOp::Gt => subject.gt(operand()),
            FilterOp::Gte => subject.gte(operand()),
            FilterOp::Lt => subject.lt(operand()),
            FilterOp::Lte => subject.lte(operand()),
            // `value` is a CBOR array → a SurrealDB array literal; `field IN [ … ]`.
            FilterOp::InSet => subject.in_(operand()),
            // A lowercased `CONTAINS`: a case-insensitive substring match.
            //
            // Not the fuzzy `~`: SurrealDB 3.0 removed those operators (3.2.3
            // answers `Unexpected token '~'`), and fuzzy was never this
            // operator's meaning anyway.
            //
            // The operand is the value's own text, not the coerced operand —
            // on the id column that would have turned the pattern into a Thing
            // and compared a record id against a substring.
            FilterOp::Like => {
                let needle = AnySurrealType::from(CborValue::Text(operand_text(value)));
                surreal_expr!(
                    "string::lowercase(<string>{}) CONTAINS string::lowercase({})",
                    (subject),
                    needle
                )
            }
            // The SurrealDB expression builder has no `NOT IN` combinator, so
            // the operator is refused and the caller filters locally.
            _ => return unsupported(Capability::FilterOps),
        };
        self.table.add_condition(expr);
        Ok(())
    }

    /// Takes a SurrealDB [`Expr`] (`Expression<AnySurrealType>`, the table's
    /// condition type), the type a Rhai `with_condition(...)` expression
    /// lowers to.
    pub(super) fn apply_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        let condition = condition.downcast::<Expr>().map_err(|other| {
            error!(
                "SurrealDB takes an Expression<AnySurrealType> as a native condition",
                key = other.key()
            )
            .mark_incorrect_usage()
        })?;
        self.table.add_condition(condition);
        Ok(())
    }

    /// Replace the quicksearch condition; `None` drops it.
    pub(super) fn apply_search(&mut self, text: Option<&str>) {
        if let Some(handle) = self.current_search_handle.take() {
            let _ = self.table.temp_remove_condition(handle);
        }
        if let Some(text) = text {
            let condition = self
                .table
                .data_source()
                .search_table_condition(&self.table, text);
            self.current_search_handle = Some(self.table.temp_add_condition(condition));
        }
    }

    /// Replace the order; `None` drops it.
    pub(super) fn apply_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        let Some((field, dir)) = order else {
            self.table.clear_orders();
            return Ok(());
        };
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for order", field = field));
        }
        self.table.clear_orders();
        let direction = match dir {
            SortDirection::Ascending => TableSortDirection::Ascending,
            SortDirection::Descending => TableSortDirection::Descending,
        };
        self.table.add_order(OrderBy {
            expression: surreal_expr!("{}", (Identifier::new(field))),
            direction,
        });
        Ok(())
    }

    /// The row just inserted as `thing`, read with the quicksearch lifted: a
    /// new row conforms to the set's conditions, but needn't match the
    /// search text. A row that still can't be read (one outside a native
    /// condition, say) is an error.
    pub(super) async fn read_back(&self, thing: &Thing) -> Result<Record<CborValue>> {
        let mut table = self.table.clone();
        if let Some(handle) = self.current_search_handle {
            let _ = table.temp_remove_condition(handle);
        }
        table
            .get_value(thing.clone())
            .await?
            .map(super::to_cbor_record)
            .ok_or_else(|| {
                error!(
                    "inserted row can't be read back through the set",
                    id = thing.to_string()
                )
            })
    }
}
