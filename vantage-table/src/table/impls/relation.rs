//! Relation expressions: an aggregate over the rows a relation gives for each
//! row of this table (a project's tag ids, whether a user favourited it, a
//! has_one's field), rendered as a correlated subquery. Roll-ups are one
//! case of it.

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_expressions::{Expression, Expressive, Order, Selectable, SelectableDataSource, expr_any};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::ReferenceKind;

use crate::{
    sorting::SortDirection,
    source::SelectSeed,
    table::{RollupOp, Table, base::ExpressionFn},
    traits::table_source::TableSource,
};

/// The rows a relation gives for the outer row, narrowed and ordered: the
/// part of `table.ref("tags").where(…).order_by(…)` before the aggregate.
#[derive(Debug, Clone)]
pub struct RelationQuery<C> {
    pub relation: String,
    pub conditions: Vec<C>,
    pub order: Vec<(String, SortDirection)>,
    pub limit: Option<i64>,
}

impl<C> RelationQuery<C> {
    pub fn new(relation: impl Into<String>) -> Self {
        Self {
            relation: relation.into(),
            conditions: Vec::new(),
            order: Vec::new(),
            limit: None,
        }
    }

    pub fn with_condition(mut self, condition: C) -> Self {
        self.conditions.push(condition);
        self
    }

    pub fn with_order(mut self, column: impl Into<String>, direction: SortDirection) -> Self {
        self.order.push((column.into(), direction));
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// What a relation expression computes over its rows. Over no rows `Count`
/// and `Sum` give 0, `Exists` false, `JoinConcat` `""` and `JoinJson` `[]`;
/// the rest give null. `Field` reads a has_one's column; every other
/// aggregate needs a has_many.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Aggregate {
    Count,
    Sum(String),
    Min(String),
    Max(String),
    Avg(String),
    Exists,
    First(String),
    JoinConcat { column: String, separator: String },
    JoinJson(String),
    Field(String),
}

impl Aggregate {
    /// The child column it reads, `None` for `Count` and `Exists`.
    pub fn column(&self) -> Option<&str> {
        match self {
            Self::Count | Self::Exists => None,
            Self::Sum(c) | Self::Min(c) | Self::Max(c) | Self::Avg(c) => Some(c),
            Self::First(c) | Self::JoinJson(c) | Self::Field(c) => Some(c),
            Self::JoinConcat { column, .. } => Some(column),
        }
    }

    /// A list built in the rows' order: without an `order_by` the order is
    /// whatever the database gives, which may change between reads.
    pub fn needs_order(&self) -> bool {
        matches!(self, Self::JoinConcat { .. } | Self::JoinJson(_))
    }

    /// The relation kind it reads.
    pub fn kind(&self) -> ReferenceKind {
        match self {
            Self::Field(_) => ReferenceKind::HasOne,
            _ => ReferenceKind::HasMany,
        }
    }

    /// The roll-up op's aggregate. `column` is required for every op but
    /// `Count`.
    pub fn from_rollup(op: RollupOp, column: Option<&str>) -> Result<Self> {
        let column = || {
            column
                .map(str::to_string)
                .ok_or_else(|| error!("roll-up needs a column", op = op))
        };
        Ok(match op {
            RollupOp::Count => Self::Count,
            RollupOp::Sum => Self::Sum(column()?),
            RollupOp::Min => Self::Min(column()?),
            RollupOp::Max => Self::Max(column()?),
            RollupOp::Avg => Self::Avg(column()?),
        })
    }

    /// The roll-up op this aggregate is, `None` for the others.
    pub fn rollup_op(&self) -> Option<RollupOp> {
        Some(match self {
            Self::Count => RollupOp::Count,
            Self::Sum(_) => RollupOp::Sum,
            Self::Min(_) => RollupOp::Min,
            Self::Max(_) => RollupOp::Max,
            Self::Avg(_) => RollupOp::Avg,
            _ => return None,
        })
    }
}

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
    /// The checks a relation expression passes before any table is built:
    /// the relation exists, has the kind the aggregate reads, is joined by
    /// one key, and an ordered list has an order.
    pub fn check_relation<C>(&self, q: &RelationQuery<C>, aggregate: &Aggregate) -> Result<()> {
        let relation = q.relation.as_str();
        let kind = self.ref_cardinality(relation)?;
        if kind != aggregate.kind() {
            let needs = match aggregate.kind() {
                ReferenceKind::HasOne => "has_one",
                _ => "has_many",
            };
            return Err(error!(
                format!("`{relation}` of `{}` isn't a {needs} relation", self.table_name()),
                relation = relation,
                table = self.table_name()
            ));
        }
        // The subquery correlates one key; the others would be dropped.
        if self.ref_keys(relation)?.len() > 1 {
            return Err(error!(
                "a relation expression's relation can't be joined by more than one key",
                relation = relation,
                table = self.table_name()
            ));
        }
        if aggregate.needs_order() && q.order.is_empty() {
            return Err(error!(
                format!("an ordered aggregate needs order_by: `{relation}`"),
                relation = relation,
                table = self.table_name()
            ));
        }
        Ok(())
    }
}

impl<T, E> Table<T, E>
where
    T: SelectableDataSource<T::Value, T::Condition> + TableSource + 'static,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String>,
    T::Column<T::AnyType>: Expressive<T::Value>,
    E: Entity<T::Value> + 'static,
{
    /// `aggregate` over the rows `q` gives for each row of this table, as an
    /// expression a query of this table can project, filter or sort by.
    pub fn relation_expression(
        &self,
        q: &RelationQuery<T::Condition>,
        aggregate: &Aggregate,
    ) -> Result<Expression<T::Value>> {
        self.check_relation(q, aggregate)?;
        self.data_source()
            .relation_expression(self, None, q, aggregate)
    }

    /// Add a read-only column `name` holding `aggregate` over the rows `q`
    /// gives for each row. Like a roll-up, the relation's target is built
    /// only when a query that includes the column is built.
    ///
    /// Errors here: an unknown relation, one of the wrong kind or joined by
    /// more than one key, an ordered list without an order. Errors when a
    /// query includes the column, and from [`Self::check_expressions`]: a
    /// target that fails to build, an unknown child column, a column that
    /// would aggregate itself, a backend that refuses the aggregate.
    pub fn with_relation_column(
        mut self,
        name: &str,
        q: RelationQuery<T::Condition>,
        aggregate: Aggregate,
    ) -> Result<Self> {
        self.check_relation(&q, &aggregate)?;
        let column = name.to_string();
        let expr_fn: ExpressionFn<T> = Arc::new(move |parent| {
            parent
                .data_source()
                .relation_expression(parent, Some(&column), &q, &aggregate)
        });
        self.expressions.insert(name.to_string(), expr_fn);
        Ok(self)
    }

    /// The relation's target narrowed to the rows of the outer row, with
    /// `q`'s conditions: the table a relation expression aggregates. Over the
    /// parent's own table it is read under `<table>_rollup`, or the subquery
    /// would correlate its own row. `name` is the column being built, so a
    /// column aggregating itself is refused.
    pub fn relation_child(
        &self,
        name: Option<&str>,
        q: &RelationQuery<T::Condition>,
        aggregate: &Aggregate,
    ) -> Result<Table<T, EmptyEntity>> {
        self.check_relation(q, aggregate)?;
        let relation = q.relation.as_str();
        let target = match name {
            Some(name) => self.rollup_target(name, relation, aggregate.column())?,
            None => self.get_ref_target_erased(relation)?,
        };
        let alias = (target.table_name() == self.table_name())
            .then(|| format!("{}_rollup", self.table_name()));
        let mut child = self.correlate_ref_target(relation, target, alias.as_deref())?;
        for condition in &q.conditions {
            child.add_condition(condition.clone());
        }
        Ok(child)
    }
}

impl<T, E> Table<T, E>
where
    T: SelectableDataSource<T::Value, T::Condition> + TableSource,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String>,
    T::Column<T::AnyType>: Expressive<T::Value>,
    E: Entity<T::Value> + 'static,
{
    /// `aggregate` over this table's rows (the child a relation expression
    /// reads), ordered and limited by `q`. The SQL shape every backend that
    /// renders subqueries shares; the lists go through
    /// [`TableSource::render_list_aggregate`].
    pub fn render_aggregate(
        &self,
        q: &RelationQuery<T::Condition>,
        aggregate: &Aggregate,
    ) -> Result<Expression<T::Value>> {
        let relation = q.relation.as_str();
        if let (Some(op), None) = (aggregate.rollup_op(), q.limit) {
            return self.aggregate_query(op, aggregate.column());
        }
        match aggregate {
            Aggregate::Exists => {
                let mut select = self.select_empty();
                select.clear_order_by();
                select.add_expression(self.data_source().expr("1", vec![]));
                Ok(expr_any!("EXISTS ({})", (select.expr())))
            }
            Aggregate::Field(column) => {
                let value = self.child_column(relation, column)?;
                Ok(expr_any!("({})", (self.select_expression(value))))
            }
            Aggregate::First(column) => {
                let mut select = self.ordered_rows(q)?;
                select.add_expression(self.child_column(relation, column)?);
                select.set_limit(Some(1), None);
                Ok(expr_any!("({})", (select.expr())))
            }
            Aggregate::JoinConcat { column, .. } | Aggregate::JoinJson(column) => {
                let mut select = self.ordered_rows(q)?;
                let source = self.data_source();
                source.add_select_column(
                    &mut select,
                    self.child_column(relation, column)?,
                    Some("v"),
                );
                let mut directions = Vec::with_capacity(q.order.len());
                for (at, (name, direction)) in q.order.iter().enumerate() {
                    let by = self.child_column(relation, name)?;
                    source.add_select_column(&mut select, by, Some(&format!("o{at}")));
                    directions.push(direction.clone());
                }
                source.render_list_aggregate(select.expr(), aggregate, &directions)
            }
            // A roll-up op over a limited set: the limit picks the rows, so
            // they are a derived table the aggregate reads.
            _ => {
                let op = aggregate.rollup_op().expect("the other aggregates are matched above");
                let mut select = self.ordered_rows(q)?;
                let value = match aggregate.column() {
                    Some(column) => self.child_column(relation, column)?,
                    None => self.data_source().expr("1", vec![]),
                };
                self.data_source()
                    .add_select_column(&mut select, value, Some("v"));
                let rows = select.expr();
                Ok(match op {
                    RollupOp::Count => expr_any!("(SELECT COUNT(*) FROM ({}) AS r)", (rows)),
                    RollupOp::Sum => expr_any!("(SELECT COALESCE(SUM(v), 0) FROM ({}) AS r)", (rows)),
                    RollupOp::Min => expr_any!("(SELECT MIN(v) FROM ({}) AS r)", (rows)),
                    RollupOp::Max => expr_any!("(SELECT MAX(v) FROM ({}) AS r)", (rows)),
                    RollupOp::Avg => expr_any!("(SELECT AVG(v) FROM ({}) AS r)", (rows)),
                })
            }
        }
    }

    /// This table's rows in `q`'s order and limit, with no fields yet.
    fn ordered_rows(&self, q: &RelationQuery<T::Condition>) -> Result<T::Select> {
        let mut select = self.select_empty();
        select.clear_fields();
        select.clear_order_by();
        for (column, direction) in &q.order {
            let by = self.child_column(&q.relation, column)?;
            let by = self.data_source().expression_condition(by)?;
            let order = match direction {
                SortDirection::Ascending => Order::Asc,
                SortDirection::Descending => Order::Desc,
            };
            select.add_order_by(by, order);
        }
        if let Some(limit) = q.limit {
            select.set_limit(Some(limit), None);
        }
        Ok(select)
    }

    /// Column `column` of this child table, its expression when it is an
    /// expression column.
    fn child_column(&self, relation: &str, column: &str) -> Result<Expression<T::Value>> {
        self.try_get_column_expr(column)?.ok_or_else(|| {
            error!(
                format!(
                    "relation `{relation}` has no column `{column}` (table `{}`)",
                    self.table_name()
                ),
                relation = relation,
                column = column,
                table = self.table_name()
            )
        })
    }
}
