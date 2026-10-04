//! `SqliteVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. SQLite advertises full read/write/count.

use super::spec::DriverBlockArgs;
use vantage_core::Result;
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    ColumnSpec, NoExtras, SpecResolver, Vista, VistaCapabilities, VistaFactory, resolve_base_spec,
};

use crate::sql_vista::{column_for_type, metadata_options};
use crate::sqlite::SqliteDB;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::types::AnySqliteType;
use crate::sqlite::vista::source::SqliteTableShell;
use crate::sqlite::vista::spec::{SqliteColumnExtras, SqliteTableExtras, SqliteVistaSpec};

/// Resolves a YAML spec by table name, so references and `base:` can rebuild
/// their tables from the live spec.
pub type SqliteSpecResolver = SpecResolver<SqliteVistaSpec>;

pub struct SqliteVistaFactory {
    db: SqliteDB,
    resolver: Option<SqliteSpecResolver>,
}

impl SqliteVistaFactory {
    pub fn new(db: SqliteDB) -> Self {
        Self { db, resolver: None }
    }

    /// Attach a spec resolver. Required for YAML-declared references to resolve
    /// their target tables — without it, a traversal yields a column-less target
    /// `Table` and the next query fails loudly.
    pub fn with_resolver(mut self, resolver: SqliteSpecResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path. The original entity
    /// type is preserved so `with_expression` closures remain typecheckable.
    pub fn from_table<E>(&self, table: Table<SqliteDB, E>) -> Result<Vista>
    where
        E: Entity<AnySqliteType> + 'static,
    {
        let name = table.table_name().to_string();
        Ok(self.wrap(table, name, false))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. Keeps the capability set and `Vista::new` call in
    /// one place so a future capability flip is a one-line edit. A query-sourced
    /// (e.g. `rhai:`) vista is `read_only`, which clears the write capabilities.
    fn wrap<E>(&self, table: Table<SqliteDB, E>, name: String, read_only: bool) -> Vista
    where
        E: Entity<AnySqliteType> + 'static,
    {
        let metadata = table.vista_metadata(metadata_options());
        let source = SqliteTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_insert: !read_only,
                can_update: !read_only,
                can_delete: !read_only,
                can_order: true,
                can_search: true,
                can_filter_operators: true,
                can_set_page_size: true,
                can_fetch_page: true,
                can_fetch_window: true,
                can_fetch_next: true,
                can_traverse_to_record: true,
                can_traverse_to_set: true,
                can_traverse_in_columns: true,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Vista::new(name, Box::new(source))
    }

    /// Build a `Table<SqliteDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver. See `build_sqlite_table`.
    pub fn table_from_spec(&self, spec: &SqliteVistaSpec) -> Result<Table<SqliteDB, EmptyEntity>> {
        build_sqlite_table(spec, self.db.clone(), self.resolver.clone())
    }
}

impl VistaFactory for SqliteVistaFactory {
    type TableExtras = SqliteTableExtras;
    type ColumnExtras = SqliteColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: SqliteVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let read_only = spec
            .driver
            .sqlite
            .as_ref()
            .is_some_and(|m| m.rhai.is_some() || m.base.is_some());
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.wrap(table, vista_name.clone(), read_only);
        vista.set_name(vista_name);
        Ok(vista)
    }
}

/// Build a `Table<SqliteDB, EmptyEntity>` from a spec. Each `references:`
/// entry rebuilds its target through `resolver` at traversal time (see
/// `Table::with_spec_references`).
pub(crate) fn build_sqlite_table(
    spec: &SqliteVistaSpec,
    db: SqliteDB,
    resolver: Option<SqliteSpecResolver>,
) -> Result<Table<SqliteDB, EmptyEntity>> {
    let block = spec.driver.sqlite.as_ref();

    if let Some(base_name) = block.and_then(|m| m.base.clone()) {
        return build_derived_table(spec, &base_name, db, resolver);
    }

    let mut table = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => table_from_rhai(spec, &code, db.clone())?,
        None => {
            let table_name = block
                .and_then(|m| m.table.clone())
                .unwrap_or_else(|| spec.name.clone());
            Table::<SqliteDB, EmptyEntity>::new(table_name, db.clone())
        }
    };

    // Dotted spec columns (`author.name`) are implicit-reference imports —
    // they are NOT plain columns (a raw `add_column` would select a
    // nonexistent identifier and read empty). They lower through
    // `with_active_columns` below, after the references they traverse are
    // registered.
    let has_dotted = spec.columns.keys().any(|n| n.contains('.'));
    for (name, col_spec) in spec.columns.iter().filter(|(n, _)| !n.contains('.')) {
        table.add_spec_column(name, col_spec, build_column)?;
    }
    table.set_spec_id_field(&spec.resolve_id_column())?;

    let mut table = table.with_spec_references(&spec.references, resolver, build_sqlite_table);

    if has_dotted {
        // Lower the dotted imports now that their relations are declared.
        // Every spec column is listed, so the plain set keeps projecting
        // unchanged while each dotted name becomes a read-only correlated
        // import aliased under the literal dotted name.
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
    spec: &SqliteVistaSpec,
    code: &str,
    db: SqliteDB,
) -> Result<Table<SqliteDB, EmptyEntity>> {
    let select = crate::sqlite::vista::rhai_source::eval_to_select_args(
        code,
        None,
        &spec.driver_block_args(),
    )?;
    Ok(Table::from_select(db, spec.name.clone(), select))
}

#[cfg(not(feature = "rhai"))]
fn table_from_rhai(
    _spec: &SqliteVistaSpec,
    _code: &str,
    _db: SqliteDB,
) -> Result<Table<SqliteDB, EmptyEntity>> {
    Err(vantage_core::error!(
        "vista declares a `rhai:` source but vantage-sql was built without the `rhai` feature"
    ))
}

/// Build a derived table: resolve `base_name` eagerly via the resolver, build
/// the base table, optionally transform its `select()` through a `rhai:` script
/// (transform mode — `base` is seeded into the engine scope), and inherit the
/// listed columns/relations (see `Table::derive_from_spec`).
fn build_derived_table(
    spec: &SqliteVistaSpec,
    base_name: &str,
    db: SqliteDB,
    resolver: Option<SqliteSpecResolver>,
) -> Result<Table<SqliteDB, EmptyEntity>> {
    let (resolver, base_spec) = resolve_base_spec(resolver, base_name)?;
    let base_table = build_sqlite_table(&base_spec, db, Some(resolver))?;

    let block = spec.driver.sqlite.as_ref();
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
    base: SqliteSelect,
    args: &[(String, String)],
) -> Result<SqliteSelect> {
    crate::sqlite::vista::rhai_source::eval_to_select_args(code, Some(base), args)
}

#[cfg(not(feature = "rhai"))]
fn eval_transform(
    _code: &str,
    _base: SqliteSelect,
    _args: &[(String, String)],
) -> Result<SqliteSelect> {
    Err(vantage_core::error!(
        "vista declares a `rhai:` transform but vantage-sql was built without the `rhai` feature"
    ))
}

/// The `sqlite.column` block names the physical column.
pub(crate) fn build_column(
    name: &str,
    col_spec: &ColumnSpec<SqliteColumnExtras>,
) -> Result<TableColumn<AnySqliteType>> {
    let physical = col_spec
        .driver
        .sqlite
        .as_ref()
        .and_then(|b| b.column.clone());
    TableColumn::from_spec(name, col_spec, physical, column_for_type)
}
