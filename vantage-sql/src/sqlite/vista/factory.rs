//! `SqliteVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. SQLite advertises full read/write/count.

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Column as VistaColumn, NoExtras, ReferenceKind, Vista, VistaCapabilities, VistaFactory,
    VistaMetadata, flags as vista_flags,
};

use crate::sqlite::SqliteDB;
use crate::sqlite::statements::SqliteSelect;
use crate::sqlite::types::AnySqliteType;
use crate::sqlite::vista::source::SqliteTableShell;
use crate::sqlite::vista::spec::{SqliteColumnExtras, SqliteTableExtras, SqliteVistaSpec};

/// Resolves a YAML spec by table name. The factory hands clones of this into
/// each `with_one` / `with_many` closure so child tables can be rebuilt from
/// the live spec at traversal time. Mirrors `SurrealSpecResolver`.
pub type SqliteSpecResolver = Arc<dyn Fn(&str) -> Option<SqliteVistaSpec> + Send + Sync>;

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
        let metadata = metadata_from_table(&table);
        let source = SqliteTableShell::new(
            table,
            VistaCapabilities {
                can_count: true,
                can_insert: !read_only,
                can_update: !read_only,
                can_delete: !read_only,
                can_order: true,
                can_search: true,
                can_set_page_size: true,
                can_fetch_page: true,
                can_fetch_window: true,
                can_fetch_next: true,
                can_traverse_to_record: true,
                can_traverse_to_set: true,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Vista::new(name, Box::new(source))
    }

    /// Build a `Table<SqliteDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver. See [`build_sqlite_table`].
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

/// Build a `Table<SqliteDB, EmptyEntity>` from a spec, registering each
/// `references:` entry as a typed `with_one` / `with_many` on the parent.
///
/// Each reference closure captures a clone of the resolver `Arc` and the target
/// table name; at traversal time it asks the resolver for the target's current
/// spec and rebuilds the child table. On a resolver miss it falls back to an
/// empty `Table::new(target_name, db)` — the next query then fails loudly when
/// it discovers no columns are defined. Mirrors `build_surreal_table`.
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

    for (name, col_spec) in &spec.columns {
        table.add_column(build_column(name, col_spec)?);
        if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
            table.add_title_field(name);
        }
        if let Some(code) = &col_spec.lazy {
            add_lazy_column(&mut table, name, code)?;
        }
    }

    let id_column = resolve_id_column(spec);
    if !table.columns().contains_key(&id_column) {
        return Err(error!(
            "id column not present in spec.columns",
            id = id_column
        ));
    }
    table.set_id_field(&id_column);

    for (rel_name, ref_spec) in &spec.references {
        let target_name = ref_spec.table.clone();
        let fk = ref_spec
            .foreign_key
            .clone()
            .unwrap_or_else(|| rel_name.clone());
        let resolver_clone = resolver.clone();

        let build_child = move |db: SqliteDB| -> Table<SqliteDB, EmptyEntity> {
            if let Some(r) = &resolver_clone
                && let Some(child_spec) = r(&target_name)
                && let Ok(child) = build_sqlite_table(&child_spec, db.clone(), Some(r.clone()))
            {
                return child;
            }
            Table::<SqliteDB, EmptyEntity>::new(target_name.clone(), db)
        };

        table = match ref_spec.kind {
            ReferenceKind::HasOne => table.with_one::<EmptyEntity>(rel_name, &fk, build_child),
            ReferenceKind::HasMany => table.with_many::<EmptyEntity>(rel_name, &fk, build_child),
        };
    }

    let table = table.with_contained_specs(&spec.contained, build_column)?;
    Ok(table)
}

/// Build a query-sourced table from a `rhai:` script.
#[cfg(feature = "rhai")]
fn table_from_rhai(
    spec: &SqliteVistaSpec,
    code: &str,
    db: SqliteDB,
) -> Result<Table<SqliteDB, EmptyEntity>> {
    let select = crate::sqlite::vista::rhai_source::eval_to_select(code, None)?;
    Ok(Table::from_select(db, spec.name.clone(), select))
}

#[cfg(not(feature = "rhai"))]
fn table_from_rhai(
    _spec: &SqliteVistaSpec,
    _code: &str,
    _db: SqliteDB,
) -> Result<Table<SqliteDB, EmptyEntity>> {
    Err(error!(
        "vista declares a `rhai:` source but vantage-sql was built without the `rhai` feature"
    ))
}

/// Build a derived table: resolve `base_name` eagerly via the resolver, build
/// the base table, optionally transform its `select()` through a `rhai:` script
/// (transform mode — `base` is seeded into the engine scope), and inherit the
/// listed columns/relations via [`Table::derive_from`]. The derived vista's own
/// `columns:` (e.g. aggregate outputs) are added on top.
fn build_derived_table(
    spec: &SqliteVistaSpec,
    base_name: &str,
    db: SqliteDB,
    resolver: Option<SqliteSpecResolver>,
) -> Result<Table<SqliteDB, EmptyEntity>> {
    let resolver = resolver.ok_or_else(|| {
        error!(
            "vista declares `base:` but no spec resolver is attached to the factory",
            base = base_name
        )
    })?;
    let base_spec = resolver(base_name)
        .ok_or_else(|| error!("base vista not found via resolver", base = base_name))?;
    let base_table = build_sqlite_table(&base_spec, db.clone(), Some(resolver.clone()))?;

    let block = spec.driver.sqlite.as_ref();
    let transformed = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => eval_transform(&code, base_table.select())?,
        None => base_table.select(),
    };

    let inherit = block.and_then(|m| m.inherit.clone()).unwrap_or_default();
    let cols: Vec<&str> = inherit.columns.iter().map(String::as_str).collect();
    let rels: Vec<&str> = inherit.relations.iter().map(String::as_str).collect();

    let mut table = Table::derive_from(
        &base_table,
        spec.name.clone(),
        move |_| transformed,
        &cols,
        &rels,
    );

    // The derived vista's own declared columns (e.g. aggregate outputs).
    for (name, col_spec) in &spec.columns {
        if !table.columns().contains_key(name) {
            table.add_column(build_column(name, col_spec)?);
        }
        if col_spec.flags.iter().any(|f| f == vista_flags::TITLE) {
            table.add_title_field(name);
        }
        if let Some(code) = &col_spec.lazy {
            add_lazy_column(&mut table, name, code)?;
        }
    }

    // Explicit id override; otherwise the id inherited from the base stands.
    if let Some(id) = &spec.id_column {
        table.set_id_field(id);
    }

    let table = table.with_contained_specs(&spec.contained, build_column)?;
    Ok(table)
}

/// Lower a column's `lazy:` script onto the table. The Rhai closure works on
/// the CBOR carrier, so each record converts per value on the way in
/// (`into_value`) and the script's result converts back on the way out
/// (`AnySqliteType::untyped`).
#[cfg(feature = "rhai")]
fn add_lazy_column(
    table: &mut Table<SqliteDB, EmptyEntity>,
    name: &str,
    code: &str,
) -> Result<()> {
    let script = vantage_vista::lazy_value_closure(code.to_string());
    table.add_lazy_expression(
        name,
        Arc::new(move |record| {
            let cbor_row: vantage_types::Record<ciborium::Value> = record
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into_value()))
                .collect();
            let script = script.clone();
            Box::pin(async move { Ok(AnySqliteType::untyped(script(&cbor_row)?)) })
        }),
    );
    Ok(())
}

#[cfg(not(feature = "rhai"))]
fn add_lazy_column(
    _table: &mut Table<SqliteDB, EmptyEntity>,
    name: &str,
    _code: &str,
) -> Result<()> {
    Err(error!(
        "column declares a `lazy:` script but vantage-sql was built without the `rhai` feature",
        column = name
    ))
}

/// Apply a `rhai:` transform to a base select. Feature-gated like
/// [`table_from_rhai`].
#[cfg(feature = "rhai")]
fn eval_transform(code: &str, base: SqliteSelect) -> Result<SqliteSelect> {
    crate::sqlite::vista::rhai_source::eval_to_select(code, Some(base))
}

#[cfg(not(feature = "rhai"))]
fn eval_transform(_code: &str, _base: SqliteSelect) -> Result<SqliteSelect> {
    Err(error!(
        "vista declares a `rhai:` transform but vantage-sql was built without the `rhai` feature"
    ))
}

pub(crate) fn resolve_id_column(spec: &SqliteVistaSpec) -> String {
    if let Some(id) = &spec.id_column {
        return id.clone();
    }
    for (name, col_spec) in &spec.columns {
        if col_spec.flags.iter().any(|f| f == vista_flags::ID) {
            return name.clone();
        }
    }
    "id".to_string()
}

pub(crate) fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<SqliteColumnExtras>,
) -> Result<TableColumn<AnySqliteType>> {
    let ty = col_spec.col_type.as_deref().unwrap_or("string");
    let alias = col_spec
        .driver
        .sqlite
        .as_ref()
        .and_then(|b| b.column.clone())
        .filter(|s| s != name);
    let hidden = col_spec.flags.iter().any(|f| f == vista_flags::HIDDEN);

    let mut col = column_for_type(name, ty)?;
    if let Some(alias) = alias {
        col = col.with_alias(alias);
    }
    if hidden {
        col = col.with_flag(ColumnFlag::Hidden);
    }
    Ok(col)
}

/// YAML type alias → typed `Column` (then erased to `Column<AnySqliteType>`).
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnySqliteType>> {
    let col: TableColumn<AnySqliteType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "decimal" | "numeric" => {
            TableColumn::from_column(TableColumn::<rust_decimal::Decimal>::new(name))
        }
        "date" => TableColumn::from_column(TableColumn::<chrono::NaiveDate>::new(name)),
        "time" => TableColumn::from_column(TableColumn::<chrono::NaiveTime>::new(name)),
        "datetime" => TableColumn::from_column(TableColumn::<chrono::NaiveDateTime>::new(name)),
        "timestamp" => {
            TableColumn::from_column(TableColumn::<chrono::DateTime<chrono::Utc>>::new(name))
        }
        other => {
            return Err(error!(
                "Unknown YAML column type",
                column = name,
                ty = other.to_string()
            ));
        }
    };
    Ok(col)
}

pub(crate) fn metadata_from_table<T, E>(table: &Table<T, E>) -> VistaMetadata
where
    T: vantage_table::traits::table_source::TableSource,
    E: Entity<T::Value>,
    T::Column<T::AnyType>: ColumnLike<T::AnyType>,
{
    let mut metadata = VistaMetadata::new();
    for (name, col) in table.columns() {
        // SQLite can ORDER BY any column server-side. Every column gets
        // the ORDERABLE flag at construction; consumers branch on it
        // before calling `Vista::add_order`.
        let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string())
            .with_flag(vista_flags::ORDERABLE);
        if col.flags().contains(&ColumnFlag::Hidden) {
            vc = vc.with_flag(vista_flags::HIDDEN);
        }
        metadata = metadata.with_column(vc);
    }
    if let Some(id_field) = table.id_field() {
        metadata = metadata.with_id_column(id_field.name().to_string());
    }
    for title in table.title_fields() {
        if let Some(col) = metadata.columns.get_mut(title) {
            col.flags.push(vista_flags::TITLE.to_string());
        }
    }
    for reference in table.vista_references() {
        metadata = metadata.with_reference(reference);
    }
    for spec in table.vista_contained() {
        metadata = metadata.with_contained(spec);
    }
    metadata
}
