//! Roll-up columns on SurrealDB.
//!
//! Over a `REFERENCE` link the child rows come from a record-reference lookup,
//! `<~(line FIELD invoice)[WHERE …]`, read off the parent record. Over any
//! other link, or before [`SurrealDB::check_references`] ran, they come from a
//! correlated subquery, `(SELECT VALUE … FROM line WHERE invoice = $parent.id)`.
//! Both feed the same aggregate, so they give the same numbers.

use vantage_core::{Result, error};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};
use vantage_table::source::SelectSource;
use vantage_table::table::{RollupOp, Table};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::ReferenceKind;

use crate::identifier::Identifier;
use crate::sum::Fx;
use crate::surrealdb::SurrealDB;
use crate::{AnySurrealType, Expr, surreal_expr};

impl SurrealDB {
    pub(crate) fn render_rollup<E: Entity<AnySurrealType> + 'static>(
        &self,
        parent: &Table<SurrealDB, E>,
        relation: &str,
        op: RollupOp,
        column: Option<&str>,
        condition: Option<Expr>,
    ) -> Result<Expr> {
        let child = parent.get_ref_target_erased(relation)?;
        let value = match op {
            RollupOp::Count => None,
            _ => {
                let name = column.ok_or_else(|| error!("roll-up needs a column", op = op))?;
                // Both forms read the stored field, not the expression.
                if child.is_calculated_column(name) {
                    return Err(error!(
                        "a roll-up can't aggregate a computed column",
                        column = name,
                        table = child.table_name()
                    ));
                }
                if !child.columns().contains_key(name) {
                    return Err(error!(
                        "roll-up column not found",
                        column = name,
                        table = child.table_name()
                    ));
                }
                Some(name)
            }
        };

        // Rows without the value are skipped, as SQL's aggregates skip NULL;
        // `math::sum` would refuse a NONE.
        let mut filters: Vec<Expr> = condition.into_iter().collect();
        if let Some(name) = value {
            filters.push(surreal_expr!(
                "{} != NONE AND {} != NULL",
                (Identifier::new(name)),
                (Identifier::new(name))
            ));
        }

        let link = parent.ref_foreign_key(relation)?;
        let has_many = parent.ref_cardinality(relation)? == ReferenceKind::HasMany;
        let checked = self.is_reference_link(child.table_name(), &link);
        // `<~` reads the stored table; a query-sourced child keeps its query
        // through the subquery.
        let stored = matches!(child.source(), SelectSource::Name(_));

        let rows = if has_many && stored && checked == Some(true) {
            referenced_rows(&child, &link, filters, value)
        } else {
            if has_many && stored {
                self.warn_slow_rollup(
                    child.table_name(),
                    &link,
                    parent.table_name(),
                    checked.is_some(),
                );
            }
            let mut child = parent.get_subquery_erased(relation)?;
            for filter in filters {
                child.add_condition(filter);
            }
            let select = child
                .select_empty()
                .with_value()
                .field(Identifier::new(value.unwrap_or("id")));
            surreal_expr!("({})", (select.expr()))
        };
        Ok(aggregate(op, rows))
    }
}

/// `<~(child FIELD link)[WHERE …].value`: the child's own conditions and
/// `filters` apply to the referencing records.
fn referenced_rows(
    child: &Table<SurrealDB, EmptyEntity>,
    link: &str,
    filters: Vec<Expr>,
    value: Option<&str>,
) -> Expr {
    let conditions: Vec<Expr> = child.conditions().cloned().chain(filters).collect();
    let mut rows = surreal_expr!(
        "<~({} FIELD {})",
        (Identifier::new(child.table_name())),
        (Identifier::new(link))
    );
    if !conditions.is_empty() {
        rows = surreal_expr!(
            "{}[WHERE {}]",
            (rows),
            (Expression::from_vec(conditions, " AND "))
        );
    }
    if let Some(name) = value {
        rows = surreal_expr!("{}.{}", (rows), (Identifier::new(name)));
    }
    rows
}

/// `op` over the array `rows`. Over no rows `count` and `math::sum` give 0 and
/// `array::min` / `array::max` NONE; `math::mean` gives NaN, so the mean is
/// taken only over a non-empty array.
fn aggregate(op: RollupOp, rows: Expr) -> Expr {
    let call = |name: &str| Fx::new(name, vec![rows.clone()]).expr();
    match op {
        RollupOp::Count => call("count"),
        RollupOp::Sum => call("math::sum"),
        RollupOp::Min => call("array::min"),
        RollupOp::Max => call("array::max"),
        RollupOp::Avg => Expression::new(
            "{ LET $rollup = {}; IF array::len($rollup) > 0 { math::mean($rollup) } ELSE { NONE } }",
            vec![ExpressiveEnum::Nested(rows)],
        ),
    }
}
