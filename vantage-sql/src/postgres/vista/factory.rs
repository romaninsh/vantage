//! `PostgresVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. PostgreSQL advertises full read/write/count.

use super::spec::DriverBlockArgs;
use vantage_core::Result;
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    ColumnSpec, NoExtras, SpecResolver, Vista, VistaCapabilities, VistaFactory, resolve_base_spec,
};

use crate::postgres::PostgresDB;
use crate::postgres::statements::PostgresSelect;
use crate::postgres::types::AnyPostgresType;
use crate::postgres::vista::source::PostgresTableShell;
use crate::postgres::vista::spec::{PostgresColumnExtras, PostgresTableExtras, PostgresVistaSpec};
use crate::sql_vista::{column_for_type, metadata_options};

/// Resolves a YAML spec by table name, so references and `base:` can rebuild
/// their tables from the live spec.
pub type PostgresSpecResolver = SpecResolver<PostgresVistaSpec>;

pub struct PostgresVistaFactory {
    db: PostgresDB,
    resolver: Option<PostgresSpecResolver>,
    notify: bool,
}

impl PostgresVistaFactory {
    pub fn new(db: PostgresDB) -> Self {
        Self {
            db,
            resolver: None,
            notify: false,
        }
    }

    /// Attach a spec resolver. Required for YAML-declared references to resolve
    /// their target tables — without it, a traversal yields a column-less target
    /// `Table` and the next query fails loudly.
    pub fn with_resolver(mut self, resolver: PostgresSpecResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Declare that this database has `{table}_changed` NOTIFY triggers
    /// installed, letting the vistas this factory builds advertise
    /// [`can_subscribe`](VistaCapabilities::can_subscribe).
    ///
    /// Off by default, and deliberately explicit: Postgres cannot tell us
    /// whether a trigger exists. `LISTEN` on a channel nobody ever notifies
    /// succeeds and then blocks forever, so a vista that advertised
    /// `can_subscribe` on the strength of being writable would promise a feed
    /// that is silent — indistinguishable, to a consumer, from a table where
    /// nothing happens. The trigger is application-installed (see `learn-10`'s
    /// `db::setup`, and the "Real-Time Push with LISTEN/NOTIFY" chapter), so
    /// the application is the only party that can honestly answer this.
    ///
    /// Applies to every table the factory builds; if only some of them carry
    /// triggers, use separate factories.
    pub fn with_notify(mut self, notify: bool) -> Self {
        self.notify = notify;
        self
    }

    pub fn from_table<E>(&self, table: Table<PostgresDB, E>) -> Result<Vista>
    where
        E: Entity<AnyPostgresType> + 'static,
    {
        let name = table.table_name().to_string();
        Ok(self.wrap(table, name, false))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. A query-sourced (e.g. `rhai:`) vista is `read_only`,
    /// which clears the write capabilities.
    fn wrap<E>(&self, table: Table<PostgresDB, E>, name: String, read_only: bool) -> Vista
    where
        E: Entity<AnyPostgresType> + 'static,
    {
        let metadata = table.vista_metadata(metadata_options());
        let source = PostgresTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_order: true,
                can_search: true,
                can_filter_operators: true,
                can_insert: !read_only,
                can_update: !read_only,
                can_delete: !read_only,
                // Push via LISTEN/NOTIFY on the `{table}_changed` channel, which
                // the application feeds from a trigger. Opt-in (`with_notify`):
                // we cannot detect the trigger, and claiming a feed we may never
                // receive is worse than claiming none.
                can_subscribe: self.notify && !read_only,
                can_set_page_size: true,
                can_fetch_page: true,
                can_fetch_next: true,
                can_fetch_window: true,
                can_traverse_to_record: true,
                can_traverse_to_set: true,
                can_traverse_in_columns: true,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Vista::new(name, Box::new(source))
    }

    /// Build a `Table<PostgresDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver. See `build_postgres_table`.
    pub fn table_from_spec(
        &self,
        spec: &PostgresVistaSpec,
    ) -> Result<Table<PostgresDB, EmptyEntity>> {
        build_postgres_table(spec, self.db.clone(), self.resolver.clone())
    }
}

impl VistaFactory for PostgresVistaFactory {
    type TableExtras = PostgresTableExtras;
    type ColumnExtras = PostgresColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: PostgresVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let read_only = spec
            .driver
            .postgres
            .as_ref()
            .is_some_and(|m| m.rhai.is_some() || m.base.is_some());
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.wrap(table, vista_name.clone(), read_only);
        vista.set_name(vista_name);
        Ok(vista)
    }
}

/// Build a `Table<PostgresDB, EmptyEntity>` from a spec. Each `references:`
/// entry rebuilds its target through `resolver` at traversal time (see
/// `Table::with_spec_references`).
pub(crate) fn build_postgres_table(
    spec: &PostgresVistaSpec,
    db: PostgresDB,
    resolver: Option<PostgresSpecResolver>,
) -> Result<Table<PostgresDB, EmptyEntity>> {
    let block = spec.driver.postgres.as_ref();

    if let Some(base_name) = block.and_then(|m| m.base.clone()) {
        return build_derived_table(spec, &base_name, db, resolver);
    }

    let mut table = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => table_from_rhai(spec, &code, db.clone())?,
        None => {
            let table_name = block
                .and_then(|m| m.table.clone())
                .unwrap_or_else(|| spec.name.clone());
            Table::<PostgresDB, EmptyEntity>::new(table_name, db.clone())
        }
    };

    // Dotted spec columns (`author.name`) are implicit-reference imports —
    // they lower through `with_active_columns` below, after the references
    // they traverse are registered; a raw `add_column` would select a
    // nonexistent identifier.
    let has_dotted = spec.columns.keys().any(|n| n.contains('.'));
    for (name, col_spec) in spec.columns.iter().filter(|(n, _)| !n.contains('.')) {
        table.add_spec_column(name, col_spec, build_column)?;
    }
    table.set_spec_id_field(&spec.resolve_id_column())?;

    let mut table = table.with_spec_references(&spec.references, resolver, build_postgres_table);

    if has_dotted {
        // Lower the dotted imports now that their relations are declared.
        let names: Vec<&str> = spec
            .columns
            .iter()
            .filter(|(_, c)| c.lazy.is_none())
            .map(|(n, _)| n.as_str())
            .collect();
        table = table.with_active_columns(&names)?;
    }

    table.with_contained_specs(&spec.contained, build_column)
}

/// Build a query-sourced table from a `rhai:` script.
#[cfg(feature = "rhai")]
fn table_from_rhai(
    spec: &PostgresVistaSpec,
    code: &str,
    db: PostgresDB,
) -> Result<Table<PostgresDB, EmptyEntity>> {
    let select = crate::postgres::vista::rhai_source::eval_to_select_args(
        code,
        None,
        &spec.driver_block_args(),
    )?;
    Ok(Table::from_select(db, spec.name.clone(), select))
}

#[cfg(not(feature = "rhai"))]
fn table_from_rhai(
    _spec: &PostgresVistaSpec,
    _code: &str,
    _db: PostgresDB,
) -> Result<Table<PostgresDB, EmptyEntity>> {
    Err(vantage_core::error!(
        "vista declares a `rhai:` source but vantage-sql was built without the `rhai` feature"
    ))
}

/// Build a derived table: resolve `base_name` eagerly via the resolver, build
/// the base table, optionally transform its `select()` through a `rhai:` script
/// (transform mode — `base` is seeded into the engine scope), and inherit the
/// listed columns/relations (see `Table::derive_from_spec`).
fn build_derived_table(
    spec: &PostgresVistaSpec,
    base_name: &str,
    db: PostgresDB,
    resolver: Option<PostgresSpecResolver>,
) -> Result<Table<PostgresDB, EmptyEntity>> {
    let (resolver, base_spec) = resolve_base_spec(resolver, base_name)?;
    let base_table = build_postgres_table(&base_spec, db, Some(resolver))?;

    let block = spec.driver.postgres.as_ref();
    let select = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => eval_transform(&code, base_table.select(), &spec.driver_block_args())?,
        None => base_table.select(),
    };
    let inherit = block.and_then(|m| m.inherit.clone()).unwrap_or_default();
    Table::derive_from_spec(
        &base_table,
        spec,
        select,
        &inherit.columns,
        &inherit.relations,
        build_column,
    )
}

/// Apply a `rhai:` transform to a base select. Feature-gated like
/// [`table_from_rhai`].
#[cfg(feature = "rhai")]
fn eval_transform(
    code: &str,
    base: PostgresSelect,
    args: &[(String, String)],
) -> Result<PostgresSelect> {
    crate::postgres::vista::rhai_source::eval_to_select_args(code, Some(base), args)
}

#[cfg(not(feature = "rhai"))]
fn eval_transform(
    _code: &str,
    _base: PostgresSelect,
    _args: &[(String, String)],
) -> Result<PostgresSelect> {
    Err(vantage_core::error!(
        "vista declares a `rhai:` transform but vantage-sql was built without the `rhai` feature"
    ))
}

/// The `postgres.column` block names the physical column.
pub(crate) fn build_column(
    name: &str,
    col_spec: &ColumnSpec<PostgresColumnExtras>,
) -> Result<TableColumn<AnyPostgresType>> {
    let physical = col_spec
        .driver
        .postgres
        .as_ref()
        .and_then(|b| b.column.clone());
    TableColumn::from_spec(name, col_spec, physical, column_for_type)
}
