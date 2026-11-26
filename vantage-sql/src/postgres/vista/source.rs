//! `PostgresTableShell` — the generic `SqlTableShell` over
//! `Table<PostgresDB, E>`. Postgres matches `Like` case-insensitively, pages
//! by the id column when no order is set, and feeds `watch` from
//! `LISTEN/NOTIFY`.

mod watch;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{Expression, Expressive};
#[cfg(feature = "rhai")]
use vantage_rhai::rhai::{Engine, EvalAltResult};
use vantage_table::column::core::Column;
use vantage_table::sorting::{OrderBy, SortDirection};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
#[cfg(feature = "rhai")]
use vantage_vista::rhai::RuleHandle;
use vantage_vista::{Transaction, Vista, VistaChangeStream};

#[cfg(feature = "rhai")]
use crate::postgres::vista::rhai_source::register_postgres_onto;
#[cfg(feature = "rhai")]
use crate::rhai_engine::RhaiExpr;

use crate::condition::{PostgresCondition, build_sql_binary};
use crate::postgres::PostgresDB;
use crate::postgres::operation::PostgresOperation;
use crate::postgres::types::AnyPostgresType;
use crate::postgres::vista::factory::PostgresVistaFactory;
use crate::primitives::identifier::ident;
use crate::sql_vista::{Shell, SqlShellDialect, SqlTableShell};

pub type PostgresTableShell<E = EmptyEntity> = SqlTableShell<PostgresDB, E, PostgresChannels>;

/// The NOTIFY channels a shell's `watch` LISTENs on, deduplicated (empty
/// means the shell does not advertise `Subscribe`), and the building
/// factory's `with_notify` flag, which relation targets inherit.
#[derive(Clone, Debug, Default)]
pub struct PostgresChannels {
    names: Vec<String>,
    notify: bool,
}

impl PostgresChannels {
    /// The `{table}_changed` channel of each table.
    pub(crate) fn of_tables<'a>(notify: bool, tables: impl IntoIterator<Item = &'a str>) -> Self {
        let mut names = Vec::new();
        for channel in tables.into_iter().map(|t| format!("{t}_changed")) {
            if !names.contains(&channel) {
                names.push(channel);
            }
        }
        Self { names, notify }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }
}

impl SqlShellDialect for PostgresDB {
    type Extra = PostgresChannels;

    const DRIVER: &'static str = "postgres";

    fn cell(value: CborValue) -> AnyPostgresType {
        AnyPostgresType::untyped(value)
    }

    fn cbor(value: AnyPostgresType) -> CborValue {
        value.into_value()
    }

    fn in_transaction(&self, tx: &Transaction) -> Result<Self> {
        PostgresDB::in_transaction(self, tx)
    }

    /// The executed form binds the inline values as `$N` parameters.
    fn preview<E: Entity<AnyPostgresType>>(table: &Table<Self, E>) -> Result<String> {
        Ok(table.try_select()?.preview())
    }

    fn column_expr(column: &Column<AnyPostgresType>) -> Expression<AnyPostgresType> {
        column.expr()
    }

    fn compare(
        subject: Expression<AnyPostgresType>,
        template: &str,
        value: AnyPostgresType,
    ) -> PostgresCondition {
        build_sql_binary::<AnyPostgresType, AnyPostgresType, _>(&subject, value, template)
    }

    fn in_list(
        subject: Expression<AnyPostgresType>,
        values: &[AnyPostgresType],
        negate: bool,
    ) -> PostgresCondition {
        if negate {
            subject.not_in_list(values)
        } else {
            subject.in_list(values)
        }
    }

    /// `ILIKE`, as quicksearch does here: case-insensitive everywhere else,
    /// so it is case-insensitive on Postgres too.
    fn like(subject: Expression<AnyPostgresType>, pattern: String) -> PostgresCondition {
        crate::postgres_expr!("{}::text ILIKE {} ESCAPE '$'", (subject), pattern).into()
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

    crate::sql_vista::scripted_dialect_hooks!(AnyPostgresType, register_postgres_onto);

    /// Targets carry the factory's `with_notify` declaration, not the
    /// parent's `Subscribe`: a query-sourced parent subscribing through
    /// `listen:` says nothing about triggers on its targets' tables. The
    /// parent's `listen:` tables are not inherited either; they describe
    /// what the parent's own rows read.
    fn target_factory<E>(
        shell: &Shell<Self, E>,
    ) -> impl Fn(Table<Self, EmptyEntity>) -> Result<Vista> + Send + Sync + 'static
    where
        E: Entity<AnyPostgresType> + 'static,
    {
        let factory = PostgresVistaFactory::new(shell.table.data_source().clone())
            .with_notify(shell.extra.notify);
        move |table| factory.from_table(table)
    }

    /// `Subscribe` is advertised only when a NOTIFY channel was declared
    /// (`with_notify` or `listen:`); `Vista::watch` refuses it otherwise.
    fn watch<E>(shell: &Shell<Self, E>) -> Result<VistaChangeStream>
    where
        E: Entity<AnyPostgresType> + 'static,
    {
        shell.listen()
    }
}

/// The PostgreSQL vocabulary for rule scripts: the `sql::` constructors and
/// operators, and `table.with_condition(<expr>)` on the [`RuleHandle`],
/// which narrows the table by the expression as a [`PostgresCondition`].
/// Register it after [`register_rule_vocab`](vantage_vista::rhai::register_rule_vocab).
#[cfg(feature = "rhai")]
pub fn register_rule_conditions(engine: &mut Engine) {
    register_postgres_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut RuleHandle,
         cond: RhaiExpr<AnyPostgresType>|
         -> std::result::Result<RuleHandle, Box<EvalAltResult>> {
            let key = cond.0.preview();
            h.native_condition(Box::new(PostgresCondition::from(cond.0)), key)
                .map_err(|e| e.to_string())?;
            Ok(h.clone())
        },
    );
    crate::sql_vista::scripted::register_rule_where_expr::<crate::postgres::PostgresDB>(engine);
}
