//! Narrowing the wrapped table: conditions, native conditions, quicksearch
//! and order. Each clause is checked before it is pushed, so a refused one
//! leaves the shell's query as it was.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::Expressive;
use vantage_table::sorting::{OrderBy, SortDirection as TableSortDirection};
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};
use vantage_vista::{
    Capability, Condition, FilterOp, NativeCondition, ReferenceKind, SortDirection, operand_text,
    unsupported,
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
    /// A dotted path (`address.city`) that is not itself a column is
    /// evaluated natively: SurrealQL walks into a nested object, or follows a
    /// record link, from a stored root column. Its root must be declared and
    /// stored.
    ///
    /// `None` when neither the field nor such a root is a column.
    fn condition_subject(&self, field: &str) -> Option<Expr> {
        if self.table.is_calculated_column(field)
            && let Some(expression) = self.table.get_column_expr(field)
        {
            return Some(surreal_expr!("({})", (expression)));
        }
        if let Some(column) = self.table.columns().get(field) {
            return Some(column.expr());
        }
        let (root, path) = field.split_once('.')?;
        if self.table.is_calculated_column(root) {
            return None;
        }
        let root = self.table.columns().get(root)?.expr();
        Some(path.split('.').fold(root, |subject, segment| {
            surreal_expr!("{}.{}", (subject), (Identifier::new(segment)))
        }))
    }

    /// `value` as `field` compares it. SurrealDB compares record ids by type,
    /// so a record id held as text (one that round-tripped through JSON or a
    /// script, or a key held by another backend's row) would match nothing:
    /// - on the id column, a `table:key` or bare key becomes this table's
    ///   record id (see `parse_id`);
    /// - on a record-link column (see `is_record_link`), a `table:key` text
    ///   becomes a record id; other text is left as it is.
    ///
    /// An `IN` list converts each element.
    fn operand(&self, field: &str, value: &CborValue) -> CborValue {
        match value {
            CborValue::Text(id) if self.metadata.id_column.as_deref().unwrap_or("id") == field => {
                self.parse_id(id).to_cbor()
            }
            CborValue::Text(_) if self.is_record_link(field) => self
                .table
                .data_source()
                .coerce_reference_value(AnySurrealType::from(value.clone()))
                .into_value(),
            CborValue::Array(items) => {
                CborValue::Array(items.iter().map(|v| self.operand(field, v)).collect())
            }
            _ => value.clone(),
        }
    }

    /// A `Thing`-typed column, or the column one of this table's has-one
    /// references reads (YAML declares links as `type: string, references:`).
    fn is_record_link(&self, field: &str) -> bool {
        let typed = self
            .table
            .columns()
            .get(field)
            .is_some_and(|column| column.get_type() == std::any::type_name::<Thing>());
        typed
            || self.metadata.references.values().any(|reference| {
                reference.kind == ReferenceKind::HasOne
                    && reference.keys.iter().any(|key| key.from == field)
            })
    }

    pub(super) fn apply_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        let subject = self
            .condition_subject(field)
            .ok_or_else(|| error!("Unknown column for condition", field = field))?;
        let value = &condition.value;
        let operand = || AnySurrealType::from(self.operand(field, value));
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
    pub(super) fn apply_search(&mut self, text: Option<&str>) -> Result<()> {
        if let Some(handle) = self.current_search_handle {
            self.table.temp_remove_condition(handle)?;
            self.current_search_handle = None;
        }
        if let Some(text) = text {
            let condition = self
                .table
                .data_source()
                .search_table_condition(&self.table, text);
            self.current_search_handle = Some(self.table.temp_add_condition(condition));
        }
        Ok(())
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
            table.temp_remove_condition(handle)?;
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
