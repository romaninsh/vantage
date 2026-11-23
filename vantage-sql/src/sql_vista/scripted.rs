//! The pieces of a SQL vista's script verbs every dialect shares:
//! `with_condition(<expr>)` and `where_expr(|t| …)` on data handles and rule
//! tables, and the native condition they narrow by.

use std::fmt::{Debug, Display};

use vantage_core::{Result, error};
use vantage_expressions::{Expression, Expressive, SelectableDataSource};
use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult, FnPtr, NativeCallContext};
use vantage_table::source::SelectSeed;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::NativeCondition;
use vantage_vista::rhai::{Handle, RuleHandle};

use super::Shell;
use super::dialect::SqlShellDialect;
use crate::primitives::identifier::Identifier;
use crate::rhai_engine::RhaiExpr;
use crate::rhai_engine::relation::{RhaiTable, register_relations};

/// The shell's table as the `table` a `where_expr` closure gets.
pub(crate) fn script_table<T, E>(shell: &Shell<T, E>) -> Option<Dynamic>
where
    T: SqlShellDialect + SelectableDataSource<T::Value, T::Condition> + 'static,
    T::Condition: From<Expression<T::Value>> + Send + Sync + 'static,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String> + From<i64> + From<f64> + From<bool> + Debug + Display + Send + Sync,
    T::Column<T::AnyType>: Expressive<T::Value>,
    Identifier: Expressive<T::Value>,
    E: Entity<T::Value> + 'static,
{
    let table: Table<T, EmptyEntity> = shell.table.clone().into_entity();
    Some(Dynamic::from(RhaiTable::new(table)))
}

/// A closure's result as the dialect's native condition.
pub(crate) fn script_condition<T>(expr: Dynamic) -> Result<NativeCondition>
where
    T: TableSource<Condition: From<Expression<T::Value>> + Send + Sync + 'static>,
    T::Value: Clone + Debug + Display + Send + Sync + 'static,
{
    let expr = expr.try_cast::<RhaiExpr<T::Value>>().ok_or_else(|| {
        error!("a `where_expr` closure returns a query expression, such as `t.ref(…).exists()`")
    })?;
    let key = expr.0.preview();
    Ok(NativeCondition::new(T::Condition::from(expr.0), key))
}

type Rhai<R> = std::result::Result<R, Box<EvalAltResult>>;

/// `self.with_condition(<expr>)` on a data handle, and the relation
/// vocabulary a `where_expr` closure's `table` needs.
pub(crate) fn register_handle_verbs<T>(engine: &mut Engine)
where
    T: SqlShellDialect + SelectableDataSource<T::Value, T::Condition> + Send + Sync + 'static,
    T::Condition: From<Expression<T::Value>> + Send + Sync + 'static,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String> + From<i64> + From<f64> + From<bool> + Debug + Display + Send + Sync,
    T::Column<T::AnyType>: Expressive<T::Value>,
    Identifier: Expressive<T::Value>,
{
    register_relations::<T>(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut Handle, cond: RhaiExpr<T::Value>| -> Rhai<Handle> {
            let key = cond.0.preview();
            h.with_native_condition("with_condition", NativeCondition::new(cond.0, key))
                .map_err(|e| e.to_string().into())
        },
    );
}

/// `table.where_expr(|t| …)` on a rule script's table: the closure gets the
/// table as narrowed so far, with the relation vocabulary, and returns the
/// boolean expression the rule narrows by.
pub(crate) fn register_rule_where_expr<T>(engine: &mut Engine)
where
    T: SqlShellDialect + SelectableDataSource<T::Value, T::Condition> + Send + Sync + 'static,
    T::Condition: From<Expression<T::Value>> + Send + Sync + 'static,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Select: Expressive<T::Value>,
    T::Value: From<String> + From<i64> + From<f64> + From<bool> + Debug + Display + Send + Sync,
    T::Column<T::AnyType>: Expressive<T::Value>,
    Identifier: Expressive<T::Value>,
{
    register_relations::<T>(engine);
    engine.register_fn(
        "where_expr",
        |ctx: NativeCallContext, h: &mut RuleHandle, f: FnPtr| -> Rhai<RuleHandle> {
            let table = h
                .native_table()
                .and_then(|t| t.downcast::<Table<T, EmptyEntity>>().ok())
                .ok_or("`where_expr` needs a backend that takes native conditions")?;
            let expr: Dynamic = f.call_within_context(&ctx, (RhaiTable::new(*table),))?;
            let expr = expr.try_cast::<RhaiExpr<T::Value>>().ok_or(
                "a `where_expr` closure returns a query expression, such as `t.ref(…).exists()`",
            )?;
            let key = expr.0.preview();
            h.native_condition(Box::new(T::Condition::from(expr.0)), key)
                .map_err(|e| e.to_string())?;
            Ok(h.clone())
        },
    );
}
