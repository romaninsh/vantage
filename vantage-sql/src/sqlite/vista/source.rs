//! `SqliteTableShell` — the generic [`SqlTableShell`] over
//! `Table<SqliteDB, E>`.
//!
//! `AnySqliteType` already wraps `ciborium::Value`, so the boundary is a
//! straight unwrap/rewrap.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::Vista;

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

    /// The executed form binds the inline values as `?N` parameters.
    fn preview<E: Entity<AnySqliteType>>(table: &Table<Self, E>) -> String {
        table.select().preview()
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
