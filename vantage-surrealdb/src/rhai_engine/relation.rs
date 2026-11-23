//! The relation vocabulary of an `expr:` script on SurrealDB: `table` and
//! `table.ref(name)`, narrowed and ordered, ending in an aggregate that
//! returns an expression (see `vantage-sql`'s for the SQL dialects).

use vantage_expressions::Expressive;
use vantage_rhai::rhai::{self, Dynamic, Engine};
use vantage_table::sorting::SortDirection;
use vantage_table::table::{Aggregate, RelationQuery, Table};
use vantage_types::EmptyEntity;

use crate::identifier::Identifier;
use crate::rhai_engine::operators::to_expr;
use crate::rhai_engine::types::RhaiExpr;
use crate::surrealdb::SurrealDB;
use crate::{Expr, surreal_expr};

/// `table` in an `expr:` script: the table the column belongs to.
#[derive(Clone)]
pub struct RhaiTable(pub Table<SurrealDB, EmptyEntity>);

/// `table.ref(name)`: the rows `name` gives for the outer row.
#[derive(Clone)]
pub struct RhaiRelation {
    table: Table<SurrealDB, EmptyEntity>,
    q: RelationQuery<Expr>,
}

type Rhai<R> = Result<R, Box<rhai::EvalAltResult>>;

fn err(message: impl Into<String>) -> Box<rhai::EvalAltResult> {
    rhai::EvalAltResult::ErrorRuntime(message.into().into(), rhai::Position::NONE).into()
}

impl RhaiRelation {
    fn with_condition(&self, condition: Expr) -> Self {
        let mut next = self.clone();
        next.q.conditions.push(condition);
        next
    }

    fn order_by(&self, column: &str, direction: &str) -> Rhai<Self> {
        let direction = match direction.to_lowercase().as_str() {
            "asc" => SortDirection::Ascending,
            "desc" => SortDirection::Descending,
            other => {
                return Err(err(format!(
                    "order_by direction must be 'asc' or 'desc', got '{other}'"
                )));
            }
        };
        let mut next = self.clone();
        next.q.order.push((column.to_string(), direction));
        Ok(next)
    }

    fn aggregate(&self, aggregate: Aggregate) -> Rhai<RhaiExpr> {
        self.table
            .relation_expression(&self.q, &aggregate)
            .map(RhaiExpr)
            .map_err(|e| err(e.to_string()))
    }
}

/// Register `ref()` on [`RhaiTable`], and the narrowing methods and
/// aggregates on [`RhaiRelation`].
pub fn register_relations(engine: &mut Engine) {
    engine.register_type_with_name::<RhaiTable>("Table");
    engine.register_type_with_name::<RhaiRelation>("Relation");

    engine.register_fn(
        "ref",
        |t: &mut RhaiTable, name: &str| -> Rhai<RhaiRelation> {
            t.0.ref_cardinality(name)
                .map_err(|_| err(format!("`{}` has no relation `{name}`", t.0.table_name())))?;
            Ok(RhaiRelation {
                table: t.0.clone(),
                q: RelationQuery::new(name),
            })
        },
    );
    engine.register_fn(
        "where",
        |r: &mut RhaiRelation, column: &str, value: Dynamic| -> Rhai<RhaiRelation> {
            let value = to_expr(value)?;
            // The column is the child's: an unknown one is an error, and an
            // expression column compares its expression.
            let child = r
                .table
                .get_ref_target_erased(&r.q.relation)
                .map_err(|e| err(e.to_string()))?;
            let target = child
                .try_get_column_expr(column)
                .map_err(|e| err(e.to_string()))?
                .ok_or_else(|| {
                    err(format!(
                        "relation `{}` has no column `{column}` (table `{}`)",
                        r.q.relation,
                        child.table_name()
                    ))
                })?;
            let target = match child.is_calculated_column(column) {
                true => surreal_expr!("({})", (target)),
                false => Identifier::new(column).expr(),
            };
            Ok(r.with_condition(surreal_expr!("{} = {}", (target), (value))))
        },
    );
    engine.register_fn("where", |r: &mut RhaiRelation, condition: RhaiExpr| {
        r.with_condition(condition.0.expr())
    });
    engine.register_fn("order_by", |r: &mut RhaiRelation, column: &str| {
        r.order_by(column, "asc")
    });
    engine.register_fn(
        "order_by",
        |r: &mut RhaiRelation, column: &str, direction: &str| r.order_by(column, direction),
    );
    engine.register_fn(
        "limit",
        |r: &mut RhaiRelation, limit: i64| -> Rhai<RhaiRelation> {
            if limit < 0 {
                return Err(err(format!("limit must be 0 or more, got {limit}")));
            }
            let mut next = r.clone();
            next.q.limit = Some(limit);
            Ok(next)
        },
    );

    engine.register_fn("count", |r: &mut RhaiRelation| {
        r.aggregate(Aggregate::Count)
    });
    engine.register_fn("exists", |r: &mut RhaiRelation| {
        r.aggregate(Aggregate::Exists)
    });
    engine.register_fn("sum", |r: &mut RhaiRelation, c: &str| {
        r.aggregate(Aggregate::Sum(c.into()))
    });
    engine.register_fn("min", |r: &mut RhaiRelation, c: &str| {
        r.aggregate(Aggregate::Min(c.into()))
    });
    engine.register_fn("max", |r: &mut RhaiRelation, c: &str| {
        r.aggregate(Aggregate::Max(c.into()))
    });
    engine.register_fn("avg", |r: &mut RhaiRelation, c: &str| {
        r.aggregate(Aggregate::Avg(c.into()))
    });
    engine.register_fn("first", |r: &mut RhaiRelation, c: &str| {
        r.aggregate(Aggregate::First(c.into()))
    });
    engine.register_fn("field", |r: &mut RhaiRelation, c: &str| {
        r.aggregate(Aggregate::Field(c.into()))
    });
    engine.register_fn("join_json", |r: &mut RhaiRelation, c: &str| {
        r.aggregate(Aggregate::JoinJson(c.into()))
    });
    engine.register_fn(
        "join_concat",
        |r: &mut RhaiRelation, c: &str, separator: &str| {
            r.aggregate(Aggregate::JoinConcat {
                column: c.into(),
                separator: separator.into(),
            })
        },
    );
}
