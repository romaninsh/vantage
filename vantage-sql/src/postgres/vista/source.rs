//! `PostgresTableShell` — the generic [`SqlTableShell`] over
//! `Table<PostgresDB, E>`. Postgres matches `Like` case-insensitively, pages
//! by the id column when no order is set, and feeds `watch` from
//! `LISTEN/NOTIFY`.

mod watch;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{Capability, Vista, VistaChangeStream};

use crate::condition::{PostgresCondition, build_sql_binary};
use crate::postgres::PostgresDB;
use crate::postgres::operation::PostgresOperation;
use crate::postgres::types::AnyPostgresType;
use crate::postgres::vista::factory::PostgresVistaFactory;
use crate::primitives::identifier::ident;
use crate::sql_vista::{Shell, SqlShellDialect, SqlTableShell};

pub type PostgresTableShell<E = EmptyEntity> = SqlTableShell<PostgresDB, E>;

impl SqlShellDialect for PostgresDB {
    type Extra = ();

    const DRIVER: &'static str = "postgres";

    fn cell(value: CborValue) -> AnyPostgresType {
        AnyPostgresType::untyped(value)
    }

    fn cbor(value: AnyPostgresType) -> CborValue {
        value.into_value()
    }

    /// The executed form binds the inline values as `$N` parameters.
    fn preview<E: Entity<AnyPostgresType>>(table: &Table<Self, E>) -> String {
        table.select().preview()
    }

    fn compare(
        column: &Column<AnyPostgresType>,
        template: &str,
        value: AnyPostgresType,
    ) -> PostgresCondition {
        build_sql_binary::<AnyPostgresType, AnyPostgresType, _>(column, value, template)
    }

    fn in_list(
        column: &Column<AnyPostgresType>,
        values: &[AnyPostgresType],
        negate: bool,
    ) -> PostgresCondition {
        if negate {
            column.not_in_list(values)
        } else {
            column.in_list(values)
        }
    }

    /// `ILIKE`, as quicksearch does here: case-insensitive everywhere else,
    /// so it is case-insensitive on Postgres too.
    fn like(field: &str, pattern: String) -> PostgresCondition {
        crate::postgres_expr!("{}::text ILIKE {} ESCAPE '$'", (ident(field)), pattern).into()
    }

    fn order_term(field: &str) -> PostgresCondition {
        crate::postgres_expr!("{}", (ident(field))).into()
    }

    /// `LIMIT`/`OFFSET` without `ORDER BY` is not a stable window: Postgres
    /// is free to return rows in any order, and a parallel or bitmap scan
    /// readily does, so successive windows can repeat a row and skip
    /// another. When the consumer has set no order of its own, page by the
    /// id column, which is unique and therefore a total order.
    ///
    /// Tables whose id is not a real column (a query-sourced view projecting
    /// a synthetic key) are left alone: ordering by a name the SELECT does
    /// not expose would fail the query outright.
    fn ordered_for_paging<E: Entity<AnyPostgresType>>(table: &Table<Self, E>) -> Table<Self, E> {
        let mut table = table.clone();
        if table.orders().next().is_some() {
            return table;
        }
        let Some(id) = table.id_field().map(|c| c.name().to_string()) else {
            return table;
        };
        if !table.columns().contains_key(&id) {
            return table;
        }
        table.add_order(OrderBy {
            expression: Self::order_term(&id),
            direction: SortDirection::Ascending,
        });
        table
    }

    /// Targets carry this shell's NOTIFY opt-in, recovered from the
    /// advertised `Subscribe` rather than stored twice. A query-sourced
    /// parent never advertises it (it is read-only), which under-advertises
    /// a real child table: the safe direction, since a missed feed degrades
    /// to the consumer's reconcile path while a phantom one degrades to
    /// silence.
    fn target_factory<E>(
        shell: &Shell<Self, E>,
    ) -> impl Fn(Table<Self, EmptyEntity>) -> Result<Vista> + Send + Sync + 'static
    where
        E: Entity<AnyPostgresType> + 'static,
    {
        let factory = PostgresVistaFactory::new(shell.table.data_source().clone())
            .with_notify(shell.capabilities.has(Capability::Subscribe));
        move |table| factory.from_table(table)
    }

    /// `Subscribe` is advertised only when the factory declared the NOTIFY
    /// triggers (`with_notify`); `Vista::watch` refuses it otherwise.
    fn watch<E>(shell: &Shell<Self, E>) -> Result<VistaChangeStream>
    where
        E: Entity<AnyPostgresType> + 'static,
    {
        shell.listen()
    }
}
