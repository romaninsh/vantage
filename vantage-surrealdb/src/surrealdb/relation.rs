//! Relation expressions on SurrealDB.
//!
//! A roll-up op without a limit reads the child rows as a roll-up does
//! (`<~line[WHERE …]` or a `$parent` subquery; see the `rollup` module), and
//! so does `exists`. An ordered or limited aggregate reads them from an
//! ordered sub-select, `(SELECT … AS v, … AS o0 FROM child WHERE … ORDER BY
//! o0 LIMIT …).v`. A has_one's field follows the record link (`tag.name`).

use vantage_core::{Result, error};
use vantage_expressions::{Expression, Expressive, Order, Selectable};
use vantage_table::sorting::SortDirection;
use vantage_table::table::{Aggregate, RelationQuery, RollupOp, Table};
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Entity};

use crate::identifier::Identifier;
use crate::sum::Fx;
use crate::surrealdb::SurrealDB;
use crate::{AnySurrealType, Expr, surreal_expr};

impl SurrealDB {
    pub(crate) fn render_relation<E: Entity<AnySurrealType> + 'static>(
        &self,
        parent: &Table<SurrealDB, E>,
        name: Option<&str>,
        q: &RelationQuery<Expr>,
        aggregate: &Aggregate,
    ) -> Result<Expr> {
        parent.check_relation(q, aggregate)?;
        let relation = q.relation.as_str();
        let condition = match q.conditions.as_slice() {
            [] => None,
            [one] => Some(one.clone()),
            many => {
                let parts: Vec<Expr> = many
                    .iter()
                    .map(|c| surreal_expr!("({})", (c.clone())))
                    .collect();
                Some(Expression::from_vec(parts, " AND "))
            }
        };
        if let Aggregate::Field(column) = aggregate {
            let target = parent.get_ref_target_erased(relation)?;
            if target.is_calculated_column(column) || !target.columns().contains_key(column) {
                return Err(error!(
                    format!(
                        "`{column}` isn't a stored column of `{}`; SurrealDB reads a has_one's \
                         field through the link",
                        target.table_name()
                    ),
                    relation = relation,
                    column = column.as_str()
                ));
            }
            let link = parent.ref_foreign_key(relation)?;
            return self
                .traversal_path_expr(&[link.as_str()], column)
                .ok_or_else(|| error!("no path for a has_one field", relation = relation));
        }
        if q.limit.is_none() {
            if let Some(op) = aggregate.rollup_op() {
                return self.render_rollup(
                    parent,
                    name,
                    relation,
                    op,
                    aggregate.column(),
                    condition,
                );
            }
            if *aggregate == Aggregate::Exists {
                let count =
                    self.render_rollup(parent, name, relation, RollupOp::Count, None, condition)?;
                return Ok(surreal_expr!("({} > 0)", (count)));
            }
        }
        let target = match name {
            Some(name) => parent.rollup_target(name, relation, aggregate.column())?,
            None => parent.get_ref_target_erased(relation)?,
        };
        self.relation_rows(parent, target, q, aggregate)
    }

    /// `aggregate` over the child rows read from an ordered sub-select: the
    /// shape every aggregate can take, used for orders, limits and computed
    /// child columns.
    pub(crate) fn relation_rows<E: Entity<AnySurrealType> + 'static>(
        &self,
        parent: &Table<SurrealDB, E>,
        target: Table<SurrealDB, EmptyEntity>,
        q: &RelationQuery<Expr>,
        aggregate: &Aggregate,
    ) -> Result<Expr> {
        let relation = q.relation.as_str();
        // `$parent` names the outer row, so a child over the parent's own
        // table needs no alias.
        let mut child = parent.correlate_ref_target(relation, target, None)?;
        for condition in &q.conditions {
            child.add_condition(condition.clone());
        }
        let column = |name: &str| {
            child.try_get_column_expr(name)?.ok_or_else(|| {
                error!(
                    format!(
                        "relation `{relation}` has no column `{name}` (table `{}`)",
                        child.table_name()
                    ),
                    relation = relation,
                    column = name
                )
            })
        };
        let value = match aggregate.column() {
            Some(name) => column(name)?,
            None => Identifier::new("id").expr(),
        };
        let mut select = child.select_empty();
        select.clear_fields();
        select.clear_order_by();
        if aggregate
            .rollup_op()
            .is_some_and(|op| op != RollupOp::Count)
        {
            // Rows without the value are skipped, as SQL's aggregates skip
            // NULL; `math::sum` would refuse a NONE.
            select.add_where_condition(surreal_expr!(
                "({}) != NONE AND ({}) != NULL",
                (value.clone()),
                (value.clone())
            ));
        }
        let mut select = select.with_expression(value, Some("v".to_string()));
        for (at, (name, direction)) in q.order.iter().enumerate() {
            let alias = format!("o{at}");
            select = select.with_expression(column(name)?, Some(alias.clone()));
            let order = match direction {
                SortDirection::Ascending => Order::Asc,
                SortDirection::Descending => Order::Desc,
            };
            select.add_order_by(Identifier::new(alias).expr(), order);
        }
        let limit = match aggregate {
            Aggregate::First(_) => Some(1),
            _ => q.limit,
        };
        if limit.is_some() {
            select.set_limit(limit, None);
        }
        let rows = surreal_expr!("({}).v", (select.expr()));
        let call = |name: &str, args: Vec<Expr>| Fx::new(name, args).expr();
        Ok(match aggregate {
            Aggregate::Exists => surreal_expr!("(count({}) > 0)", (rows)),
            Aggregate::First(_) => call("array::first", vec![rows]),
            Aggregate::JoinJson(_) => rows,
            Aggregate::JoinConcat { separator, .. } => {
                let separator = AnySurrealType::from(separator.clone());
                call("array::join", vec![rows, surreal_expr!("{}", separator)])
            }
            other => {
                let op = other
                    .rollup_op()
                    .ok_or_else(|| error!(format!("SurrealDB can't render {other:?}")))?;
                super::rollup::aggregate(op, rows)
            }
        })
    }
}
