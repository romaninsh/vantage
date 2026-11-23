//! `SqliteTableShell` — the generic `SqlTableShell` over
//! `Table<SqliteDB, E>`.
//!
//! `AnySqliteType` already wraps `ciborium::Value`, so the boundary is a
//! straight unwrap/rewrap.

use ciborium::Value as CborValue;
use vantage_core::Result;
#[cfg(feature = "rhai")]
use vantage_rhai::rhai::{Engine, EvalAltResult};
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
#[cfg(feature = "rhai")]
use vantage_vista::rhai::RuleHandle;
use vantage_vista::{NativeCondition, Transaction, Vista};

#[cfg(feature = "rhai")]
use crate::rhai_engine::RhaiExpr;
#[cfg(feature = "rhai")]
use crate::sqlite::vista::rhai_source::register_sqlite_onto;

use crate::condition::{SqliteCondition, build_sql_binary};
use crate::primitives::identifier::ident;
use crate::sql_vista::{Shell, SqlShellDialect, SqlTableShell};
use crate::sqlite::SqliteDB;
use crate::sqlite::operation::SqliteOperation;
use crate::sqlite::types::AnySqliteType;
use crate::sqlite::vista::factory::SqliteVistaFactory;

pub type SqliteTableShell<E = EmptyEntity> = SqlTableShell<SqliteDB, E>;

impl SqlShellDialect for SqliteDB {
    type Extra = ();

    const DRIVER: &'static str = "sqlite";

    fn cell(value: CborValue) -> AnySqliteType {
        AnySqliteType::untyped(value)
    }

    fn cbor(value: AnySqliteType) -> CborValue {
        value.into_value()
    }

    fn in_transaction(&self, tx: &Transaction) -> Result<Self> {
        SqliteDB::in_transaction(self, tx)
    }

    /// The executed form binds the inline values as `?N` parameters.
    fn preview<E: Entity<AnySqliteType>>(table: &Table<Self, E>) -> Result<String> {
        Ok(table.try_select()?.preview())
    }

    fn compare(
        column: &Column<AnySqliteType>,
        template: &str,
        value: AnySqliteType,
    ) -> SqliteCondition {
        build_sql_binary::<AnySqliteType, AnySqliteType, _>(column, value, template)
    }

    fn in_list(
        column: &Column<AnySqliteType>,
        values: &[AnySqliteType],
        negate: bool,
    ) -> SqliteCondition {
        if negate {
            column.not_in_list(values)
        } else {
            column.in_list(values)
        }
    }

    fn like(field: &str, pattern: String) -> SqliteCondition {
        crate::sqlite_expr!("{} LIKE {} ESCAPE '$'", (ident(field)), pattern).into()
    }

    fn order_term(field: &str) -> SqliteCondition {
        crate::sqlite_expr!("{}", (ident(field))).into()
    }

    /// Windows run in the table's own order.
    fn ordered_for_paging<E: Entity<AnySqliteType>>(table: &Table<Self, E>) -> Table<Self, E> {
        table.clone()
    }

    /// Takes the dialect's own condition or a query expression.
    fn add_native_condition<E>(shell: &mut Shell<Self, E>, condition: NativeCondition) -> Result<()>
    where
        E: Entity<AnySqliteType> + 'static,
    {
        crate::sql_vista::add_native_condition(shell, condition)
    }

    /// The SQLite query vocabulary, `self.with_condition(<expr>)`, and the
    /// relation vocabulary a `where_expr` closure's `table` reads.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(engine: &mut vantage_rhai::rhai::Engine) {
        register_sqlite_onto(engine);
        crate::sql_vista::scripted::register_handle_verbs::<Self>(engine);
    }

    #[cfg(feature = "rhai")]
    fn script_table<E>(shell: &Shell<Self, E>) -> Option<vantage_rhai::rhai::Dynamic>
    where
        E: Entity<AnySqliteType> + 'static,
    {
        crate::sql_vista::scripted::script_table(shell)
    }

    #[cfg(feature = "rhai")]
    fn script_condition(
        expr: vantage_rhai::rhai::Dynamic,
    ) -> Result<vantage_vista::NativeCondition> {
        crate::sql_vista::scripted::script_condition::<Self>(expr)
    }

    fn target_factory<E>(
        shell: &Shell<Self, E>,
    ) -> impl Fn(Table<Self, EmptyEntity>) -> Result<Vista> + Send + Sync + 'static
    where
        E: Entity<AnySqliteType> + 'static,
    {
        let factory = SqliteVistaFactory::new(shell.table.data_source().clone());
        move |table| factory.from_table(table)
    }
}

/// The SQLite vocabulary for rule scripts: the `sql::` constructors and
/// operators, and `table.with_condition(<expr>)` on the [`RuleHandle`],
/// which narrows the table by the expression as a [`SqliteCondition`].
/// Register it after [`register_rule_vocab`](vantage_vista::rhai::register_rule_vocab).
#[cfg(feature = "rhai")]
pub fn register_rule_conditions(engine: &mut Engine) {
    register_sqlite_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut RuleHandle,
         cond: RhaiExpr<AnySqliteType>|
         -> std::result::Result<RuleHandle, Box<EvalAltResult>> {
            let key = cond.0.preview();
            h.native_condition(Box::new(SqliteCondition::from(cond.0)), key)
                .map_err(|e| e.to_string())?;
            Ok(h.clone())
        },
    );
    crate::sql_vista::scripted::register_rule_where_expr::<crate::sqlite::SqliteDB>(engine);
}
