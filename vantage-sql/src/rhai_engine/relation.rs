//! The relation vocabulary of an `expr:` script: `table`, this table with
//! the outer row correlated, and `table.ref(name)`, a related set that
//! narrows, orders and ends in an aggregate returning an expression:
//!
//! ```rhai
//! table.ref("tags").order_by("tag_name").join_json("project_tags_id")
//! table.ref("favorites").where("user_id", session.uid).exists()
//! ```
//!
//! Each aggregate renders through [`Table::relation_expression`], so the
//! subquery is the one a Rust `with_relation_column` builds.

use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, SelectableDataSource, expr_any};
use vantage_rhai::rhai::{self, Dynamic, Engine};
use vantage_table::sorting::SortDirection;
use vantage_table::source::SelectSeed;
use vantage_table::table::{Aggregate, RelationQuery, Table};
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;

use super::RhaiExpr;
use super::convert::rhai_err;
use crate::primitives::identifier::{Identifier, ident};

/// `table` in an `expr:` script: the table the column belongs to.
pub struct RhaiTable<T: TableSource>(pub Table<T, EmptyEntity>);

impl<T: TableSource> Clone for RhaiTable<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: TableSource> RhaiTable<T> {
    pub fn new(table: Table<T, EmptyEntity>) -> Self {
        Self(table)
    }
}

/// `table.ref(name)`: the rows `name` gives for the outer row, narrowed and
/// ordered so far.
pub struct RhaiRelation<T: TableSource> {
    table: Table<T, EmptyEntity>,
    q: RelationQuery<T::Condition>,
}

impl<T: TableSource> Clone for RhaiRelation<T> {
    fn clone(&self) -> Self {
        Self {
            table: self.table.clone(),
            q: self.q.clone(),
        }
    }
}

type Rhai<R> = Result<R, Box<rhai::EvalAltResult>>;

/// A Rhai value as a cell: a scalar binds as a parameter.
fn scalar<V>(value: &Dynamic) -> Rhai<V>
where
    V: From<i64> + From<f64> + From<bool> + From<String>,
{
    if let Some(v) = value.clone().try_cast::<i64>() {
        Ok(V::from(v))
    } else if let Some(v) = value.clone().try_cast::<f64>() {
        Ok(V::from(v))
    } else if let Some(v) = value.clone().try_cast::<bool>() {
        Ok(V::from(v))
    } else if let Some(v) = value.clone().try_cast::<rhai::ImmutableString>() {
        Ok(V::from(v.to_string()))
    } else if let Some(v) = value.clone().try_cast::<String>() {
        Ok(V::from(v))
    } else {
        Err(rhai_err(format!(
            "where(column, value) takes a scalar or a query expression, got '{}'",
            super::convert::friendly_type_name(value)
        )))
    }
}

impl<T> RhaiRelation<T>
where
    T: SelectableDataSource<T::Value, T::Condition> + TableSource + 'static,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String> + From<i64> + From<f64> + From<bool> + Debug + Display,
    T::Column<T::AnyType>: Expressive<T::Value>,
    Identifier: Expressive<T::Value>,
{
    fn condition(&self, expr: Expression<T::Value>) -> Rhai<T::Condition> {
        self.table
            .data_source()
            .expression_condition(expr)
            .map_err(|e| rhai_err(e.to_string()))
    }

    fn where_eq(mut self, column: &str, value: Dynamic) -> Rhai<Self> {
        // The column is the child's: an unknown one is an error, not a
        // reference that SQL would resolve to the outer table's column.
        let relation = self.q.relation.clone();
        let child = self
            .table
            .get_ref_target_erased(&relation)
            .map_err(|e| rhai_err(e.to_string()))?;
        let computed = child.is_calculated_column(column);
        let target = child
            .try_get_column_expr(column)
            .map_err(|e| rhai_err(e.to_string()))?
            .ok_or_else(|| {
                rhai_err(format!(
                    "relation `{relation}` has no column `{column}` (table `{}`)",
                    child.table_name()
                ))
            })?;
        let condition = match (value.clone().try_cast::<RhaiExpr<T::Value>>(), computed) {
            (Some(e), false) => self.condition(expr_any!("{} = ({})", (ident(column)), (e.0)))?,
            (Some(e), true) => self.condition(expr_any!("({}) = ({})", (target), (e.0)))?,
            (None, false) => self
                .table
                .data_source()
                .eq_value_condition(column, scalar::<T::Value>(&value)?)
                .map_err(|e| rhai_err(e.to_string()))?,
            (None, true) => {
                let value = scalar::<T::Value>(&value)?;
                self.condition(expr_any!("({}) = {}", (target), value))?
            }
        };
        self.q.conditions.push(condition);
        Ok(self)
    }

    fn where_expr(mut self, condition: RhaiExpr<T::Value>) -> Rhai<Self> {
        let condition = self.condition(condition.0)?;
        self.q.conditions.push(condition);
        Ok(self)
    }

    fn order_by(mut self, column: &str, direction: &str) -> Rhai<Self> {
        let direction = match direction.to_lowercase().as_str() {
            "asc" => SortDirection::Ascending,
            "desc" => SortDirection::Descending,
            other => {
                return Err(rhai_err(format!(
                    "order_by direction must be 'asc' or 'desc', got '{other}'"
                )));
            }
        };
        self.q.order.push((column.to_string(), direction));
        Ok(self)
    }

    fn limit(mut self, limit: i64) -> Rhai<Self> {
        if limit < 0 {
            return Err(rhai_err(format!("limit must be 0 or more, got {limit}")));
        }
        self.q.limit = Some(limit);
        Ok(self)
    }

    fn aggregate(&self, aggregate: Aggregate) -> Rhai<RhaiExpr<T::Value>> {
        self.table
            .relation_expression(&self.q, &aggregate)
            .map(RhaiExpr)
            .map_err(|e| rhai_err(e.to_string()))
    }
}

/// Register `ref()` on [`RhaiTable`], and the narrowing methods and
/// aggregates on [`RhaiRelation`], for backend `T`.
pub fn register_relations<T>(engine: &mut Engine)
where
    T: SelectableDataSource<T::Value, T::Condition> + TableSource + Send + Sync + 'static,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String> + From<i64> + From<f64> + From<bool> + Debug + Display + Send + Sync,
    T::Condition: Send + Sync,
    T::Column<T::AnyType>: Expressive<T::Value>,
    Identifier: Expressive<T::Value>,
{
    type Rel<T> = RhaiRelation<T>;
    type Ex<T> = RhaiExpr<<T as TableSource>::Value>;

    engine.register_type_with_name::<RhaiTable<T>>("Table");
    engine.register_type_with_name::<Rel<T>>("Relation");

    engine.register_fn("ref", |t: &mut RhaiTable<T>, name: &str| -> Rhai<Rel<T>> {
        t.0.ref_cardinality(name)
            .map_err(|_| rhai_err(format!("`{}` has no relation `{name}`", t.0.table_name())))?;
        Ok(RhaiRelation {
            table: t.0.clone(),
            q: RelationQuery::new(name),
        })
    });

    engine.register_fn("where", |r: &mut Rel<T>, column: &str, value: Dynamic| {
        r.clone().where_eq(column, value)
    });
    engine.register_fn("where", |r: &mut Rel<T>, condition: Ex<T>| {
        r.clone().where_expr(condition)
    });
    engine.register_fn("order_by", |r: &mut Rel<T>, column: &str| {
        r.clone().order_by(column, "asc")
    });
    engine.register_fn(
        "order_by",
        |r: &mut Rel<T>, column: &str, direction: &str| r.clone().order_by(column, direction),
    );
    engine.register_fn("limit", |r: &mut Rel<T>, limit: i64| r.clone().limit(limit));

    engine.register_fn("count", |r: &mut Rel<T>| r.aggregate(Aggregate::Count));
    engine.register_fn("exists", |r: &mut Rel<T>| r.aggregate(Aggregate::Exists));
    engine.register_fn("sum", |r: &mut Rel<T>, c: &str| {
        r.aggregate(Aggregate::Sum(c.into()))
    });
    engine.register_fn("min", |r: &mut Rel<T>, c: &str| {
        r.aggregate(Aggregate::Min(c.into()))
    });
    engine.register_fn("max", |r: &mut Rel<T>, c: &str| {
        r.aggregate(Aggregate::Max(c.into()))
    });
    engine.register_fn("avg", |r: &mut Rel<T>, c: &str| {
        r.aggregate(Aggregate::Avg(c.into()))
    });
    engine.register_fn("first", |r: &mut Rel<T>, c: &str| {
        r.aggregate(Aggregate::First(c.into()))
    });
    engine.register_fn("field", |r: &mut Rel<T>, c: &str| {
        r.aggregate(Aggregate::Field(c.into()))
    });
    engine.register_fn("join_json", |r: &mut Rel<T>, c: &str| {
        r.aggregate(Aggregate::JoinJson(c.into()))
    });
    engine.register_fn("join_concat", |r: &mut Rel<T>, c: &str, separator: &str| {
        r.aggregate(Aggregate::JoinConcat {
            column: c.into(),
            separator: separator.into(),
        })
    });
}

/// The relation vocabulary of every SQL dialect this build has, for a host
/// that runs data scripts over tables of any of them: the `table` a
/// `where_expr(|t| …)` closure gets is the dialect's own, so each needs its
/// methods.
pub fn register_relation_vocab(engine: &mut Engine) {
    #[cfg(feature = "sqlite")]
    register_relations::<crate::sqlite::SqliteDB>(engine);
    #[cfg(feature = "postgres")]
    register_relations::<crate::postgres::PostgresDB>(engine);
    #[cfg(feature = "mysql")]
    register_relations::<crate::mysql::MysqlDB>(engine);
}

/// Evaluate a column's `expr:` script on `host` into the expression the
/// column projects. With `table`, the script has it as `table`, so it can
/// build a relation expression; `env` carries the caller's other variables
/// (`session`, `app`). Each dialect's `eval_column_expr` calls this with its
/// own host.
pub fn eval_column_expr_on<T>(
    host: &vantage_rhai::Host,
    code: &str,
    table: Option<Table<T, EmptyEntity>>,
    env: vantage_rhai::Env,
) -> vantage_core::Result<Expression<T::Value>>
where
    T: TableSource + Send + Sync + 'static,
    T::Value: Clone + Send + Sync + 'static,
    T::Condition: Send + Sync,
{
    let env = match table {
        Some(table) => env.var("table", Dynamic::from(RhaiTable::new(table))),
        None => env,
    };
    host.compile(&vantage_rhai::Block::from(code))
        .and_then(|script| script.eval_as::<RhaiExpr<T::Value>>(&env))
        .map(|expr| expr.0)
        .map_err(|e| vantage_core::error!(format!("expr: {e}"), detail = e.to_string()))
}
