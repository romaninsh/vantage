//! `MysqlTableShell` — the generic `SqlTableShell` over `Table<MysqlDB, E>`.
//! MySQL adds native conditions, transactional import and, with `rhai`,
//! its expression vocabulary and scripted reference traversal. The shell
//! carries its factory (spec resolver and build injector included) for the
//! traversal targets it builds.

mod import;
#[cfg(feature = "rhai")]
mod scripted;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity, Record};
use vantage_vista::{NativeCondition, Reference, Transaction, Vista};

use crate::condition::{MysqlCondition, build_sql_binary};
use crate::mysql::MysqlDB;
use crate::mysql::operation::MysqlOperation;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::factory::MysqlVistaFactory;
use crate::primitives::identifier::ident;
use crate::sql_vista::{Shell, SqlShellDialect, SqlTableShell};

#[cfg(feature = "rhai")]
pub(crate) use scripted::register_extensions;
#[cfg(feature = "rhai")]
pub use scripted::register_rule_conditions;

/// The shell carries the factory that built it, with its spec resolver and
/// build injector, for the targets and scripts it builds in turn.
pub type MysqlTableShell<E = EmptyEntity> = SqlTableShell<MysqlDB, E, MysqlVistaFactory>;

impl<E> MysqlTableShell<E>
where
    E: Entity<AnyMysqlType>,
{
    fn factory(&self) -> MysqlVistaFactory {
        self.extra.clone()
    }
}

#[async_trait]
impl SqlShellDialect for MysqlDB {
    type Extra = MysqlVistaFactory;

    const DRIVER: &'static str = "mysql";

    fn cell(value: CborValue) -> AnyMysqlType {
        AnyMysqlType::untyped(value)
    }

    fn cbor(value: AnyMysqlType) -> CborValue {
        value.into_value()
    }

    fn in_transaction(&self, tx: &Transaction) -> Result<Self> {
        MysqlDB::in_transaction(self, tx)
    }

    /// The executed form binds the inline values as `?` parameters.
    fn preview<E: Entity<AnyMysqlType>>(table: &Table<Self, E>) -> String {
        table.select().preview()
    }

    fn compare(
        column: &Column<AnyMysqlType>,
        template: &str,
        value: AnyMysqlType,
    ) -> MysqlCondition {
        build_sql_binary::<AnyMysqlType, AnyMysqlType, _>(column, value, template)
    }

    fn in_list(
        column: &Column<AnyMysqlType>,
        values: &[AnyMysqlType],
        negate: bool,
    ) -> MysqlCondition {
        if negate {
            column.not_in_list(values)
        } else {
            column.in_list(values)
        }
    }

    fn like(field: &str, pattern: String) -> MysqlCondition {
        crate::mysql_expr!("{} LIKE {} ESCAPE '$'", (ident(field)), pattern).into()
    }

    fn order_term(field: &str) -> MysqlCondition {
        crate::mysql_expr!("{}", (ident(field))).into()
    }

    /// `LIMIT`/`OFFSET` without `ORDER BY` is not a stable window, and a sort
    /// on a non-unique column leaves ties in unspecified order, so successive
    /// windows could repeat or skip rows. The id column is unique, so it is
    /// appended as the final tiebreak (or becomes the whole order when the
    /// consumer set none). Tables without an id column are left alone.
    fn ordered_for_paging<E: Entity<AnyMysqlType>>(table: &Table<Self, E>) -> Table<Self, E> {
        let mut table = table.clone();
        let Some(id) = table.id_field().map(|c| c.name().to_string()) else {
            return table;
        };
        table.add_order(OrderBy {
            expression: Self::order_term(&id),
            direction: SortDirection::Ascending,
        });
        table
    }

    fn target_factory<E>(
        shell: &Shell<Self, E>,
    ) -> impl Fn(Table<Self, EmptyEntity>) -> Result<Vista> + Send + Sync + 'static
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        let factory = shell.factory();
        move |table| factory.from_table(table)
    }

    /// A reference carrying a Rhai `build_script` builds its related rows
    /// through the script engine instead of the join keys. The bare target
    /// (no row) is the same table either way.
    fn join_target<E>(
        shell: &Shell<Self, E>,
        relation: &str,
        reference: &Reference,
        row: Option<&Record<CborValue>>,
    ) -> Result<Vista>
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        #[cfg(feature = "rhai")]
        if let (Some(script), Some(row)) = (reference.build_script.as_deref(), row) {
            return shell.get_ref_via_script(script, row);
        }
        shell.join_target(relation, reference, row)
    }

    /// Takes a [`MysqlCondition`] or the `Expression<AnyMysqlType>` the Rhai
    /// vocabulary's expression constructors produce.
    fn add_native_condition<E>(shell: &mut Shell<Self, E>, condition: NativeCondition) -> Result<()>
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        let condition = match condition.downcast::<MysqlCondition>() {
            Ok(condition) => condition,
            Err(other) => match other.downcast::<Expression<AnyMysqlType>>() {
                Ok(expr) => MysqlCondition::from(expr),
                Err(other) => {
                    return Err(error!(
                        "MySQL takes a MysqlCondition or an Expression<AnyMysqlType> as a native condition",
                        key = other.key()
                    )
                    .mark_incorrect_usage());
                }
            },
        };
        shell.table.add_condition(condition);
        Ok(())
    }

    /// A query-sourced vista doesn't advertise `Import`, so `Vista::import`
    /// refuses it before the shell is asked.
    async fn import<E>(
        shell: &Shell<Self, E>,
        records: &IndexMap<String, Record<CborValue>>,
    ) -> Result<usize>
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        import::import(shell, records).await
    }

    /// The MySQL query vocabulary (`sql::select`, `sql::ident`, `sql::expr`,
    /// operators, …) plus `self.with_condition(<expr>)`, which pushes the
    /// expression through `add_native_condition`.
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(engine: &mut vantage_rhai::rhai::Engine) {
        register_extensions(engine);
    }
}
