//! `MysqlVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. MySQL advertises read, write, order, search,
//! windowed reads, native conditions, transactional import and traversal
//! (scripted references with `rhai`); a query-sourced vista is read-only.

#[cfg(feature = "rhai")]
mod scripting;

use super::spec::DriverBlockArgs;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{Capability, ColumnSpec, SpecResolver, Vista, VistaFactory, resolve_base_spec};

use crate::mysql::MysqlDB;
use crate::mysql::statements::MysqlSelect;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::source::MysqlTableShell;
use crate::mysql::vista::spec::{
    MysqlColumnExtras, MysqlReferenceExtras, MysqlTableExtras, MysqlVistaSpec,
};
use crate::sql_vista::{column_for_type, metadata_options, sql_capabilities};

/// Resolves a YAML spec by table name, so references and `base:` can rebuild
/// their tables from the live spec.
pub type MysqlSpecResolver = SpecResolver<MysqlVistaSpec>;

#[derive(Clone)]
pub struct MysqlVistaFactory {
    db: MysqlDB,
    resolver: Option<MysqlSpecResolver>,
}

impl MysqlVistaFactory {
    pub fn new(db: MysqlDB) -> Self {
        Self { db, resolver: None }
    }

    /// Attach a spec resolver. Required for YAML-declared references to resolve
    /// their target tables — without it, a traversal yields a column-less target
    /// `Table` and the next query fails loudly.
    pub fn with_resolver(mut self, resolver: MysqlSpecResolver) -> Self {
        self.resolver = Some(resolver);
        self
    }

    pub fn from_table<E>(&self, table: Table<MysqlDB, E>) -> Result<Vista>
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        let name = table.table_name().to_string();
        self.wrap(table, name, false, IndexMap::new())
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. A query-sourced (e.g. `rhai:`) vista is `read_only`,
    /// which clears the write capabilities. `ref_scripts` maps a reference
    /// name to its Rhai build-script; each must name a reference the table
    /// declares.
    fn wrap<E>(
        &self,
        table: Table<MysqlDB, E>,
        name: String,
        read_only: bool,
        ref_scripts: IndexMap<String, String>,
    ) -> Result<Vista>
    where
        E: Entity<AnyMysqlType> + 'static,
    {
        let mut metadata = table.vista_metadata(metadata_options());
        for (relation, script) in ref_scripts {
            let reference = metadata.references.get_mut(&relation).ok_or_else(|| {
                error!(
                    "reference script names a reference the table does not declare",
                    relation = relation
                )
            })?;
            reference.build_script = Some(script);
        }
        // Native conditions narrow a read-only vista too; import is a write.
        let mut capabilities = sql_capabilities(read_only).with(Capability::NativeConditions);
        if !read_only {
            capabilities = capabilities.with(Capability::Import);
        }
        let shell = MysqlTableShell::new(table, capabilities, metadata, self.resolver.clone());
        Ok(Vista::new(name, shell))
    }

    /// Build a `Table<MysqlDB, EmptyEntity>` from a spec, resolving any
    /// `references:` against the attached resolver. See `build_mysql_table`.
    pub fn table_from_spec(&self, spec: &MysqlVistaSpec) -> Result<Table<MysqlDB, EmptyEntity>> {
        build_mysql_table(spec, self.db.clone(), self.resolver.clone())
    }
}

impl VistaFactory for MysqlVistaFactory {
    type TableExtras = MysqlTableExtras;
    type ColumnExtras = MysqlColumnExtras;
    type ReferenceExtras = MysqlReferenceExtras;

    fn build_from_spec(&self, spec: MysqlVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let read_only = spec
            .driver
            .mysql
            .as_ref()
            .is_some_and(|m| m.rhai.is_some() || m.base.is_some());
        let ref_scripts: IndexMap<String, String> = spec
            .references
            .iter()
            .filter_map(|(name, r)| {
                let script = r.driver.mysql.as_ref()?.rhai.clone()?;
                Some((name.clone(), script))
            })
            .collect();
        let modify = spec.driver.mysql.as_ref().and_then(|m| m.modify.clone());
        #[cfg(not(feature = "rhai"))]
        if let Some(relation) = ref_scripts.keys().next() {
            return Err(error!(
                "reference declares a `rhai:` build script but vantage-sql was built without the `rhai` feature",
                vista = vista_name.as_str(),
                relation = relation.as_str()
            ));
        }
        #[cfg(not(feature = "rhai"))]
        if modify.is_some() {
            return Err(error!(
                "vista declares a `modify:` script but vantage-sql was built without the `rhai` feature",
                vista = vista_name.as_str()
            ));
        }

        let table = self.table_from_spec(&spec)?;
        #[cfg_attr(not(feature = "rhai"), allow(unused_mut))]
        let mut vista = self.wrap(table, vista_name, read_only, ref_scripts)?;

        // Final step: a `mysql: { modify }` script tweaks the built vista, so it
        // composes with `table`/`rhai`/`base`.
        #[cfg(feature = "rhai")]
        if let Some(code) = modify {
            vista = self.apply_modify(vista, &code)?;
        }
        Ok(vista)
    }
}

/// Build a `Table<MysqlDB, EmptyEntity>` from a spec. Each `references:`
/// entry rebuilds its target through `resolver` at traversal time (see
/// `Table::with_spec_references`).
pub(crate) fn build_mysql_table(
    spec: &MysqlVistaSpec,
    db: MysqlDB,
    resolver: Option<MysqlSpecResolver>,
) -> Result<Table<MysqlDB, EmptyEntity>> {
    let block = spec.driver.mysql.as_ref();

    if let Some(base_name) = block.and_then(|m| m.base.clone()) {
        return build_derived_table(spec, &base_name, db, resolver);
    }

    let mut table = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => table_from_rhai(spec, &code, db.clone())?,
        None => {
            let table_name = block
                .and_then(|m| m.table.clone())
                .unwrap_or_else(|| spec.name.clone());
            Table::<MysqlDB, EmptyEntity>::new(table_name, db.clone())
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

    let mut table = table.with_spec_references(&spec.references, resolver, build_mysql_table)?;

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
    spec: &MysqlVistaSpec,
    code: &str,
    db: MysqlDB,
) -> Result<Table<MysqlDB, EmptyEntity>> {
    let select = crate::mysql::vista::rhai_source::eval_to_select_args(
        code,
        None,
        &spec.driver_block_args(),
    )?;
    Ok(Table::from_select(db, spec.name.clone(), select))
}

#[cfg(not(feature = "rhai"))]
fn table_from_rhai(
    _spec: &MysqlVistaSpec,
    _code: &str,
    _db: MysqlDB,
) -> Result<Table<MysqlDB, EmptyEntity>> {
    Err(vantage_core::error!(
        "vista declares a `rhai:` source but vantage-sql was built without the `rhai` feature"
    ))
}

/// Build a derived table: resolve `base_name` eagerly via the resolver, build
/// the base table, optionally transform its `select()` through a `rhai:` script
/// (transform mode — `base` is seeded into the engine scope), inherit the
/// listed columns/relations (see `Table::derive_from_spec`) and add the spec's
/// own references.
fn build_derived_table(
    spec: &MysqlVistaSpec,
    base_name: &str,
    db: MysqlDB,
    resolver: Option<MysqlSpecResolver>,
) -> Result<Table<MysqlDB, EmptyEntity>> {
    let (resolver, base_spec) = resolve_base_spec(resolver, base_name)?;
    let base_table = build_mysql_table(&base_spec, db, Some(resolver.clone()))?;

    let block = spec.driver.mysql.as_ref();
    let select = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => eval_transform(&code, base_table.select(), &spec.driver_block_args())?,
        None => base_table.select(),
    };
    let inherit = block.and_then(|m| m.inherit.clone()).unwrap_or_default();
    let table = Table::derive_from_spec(
        &base_table,
        spec,
        select,
        &inherit.columns,
        &inherit.relations,
        build_column,
    )?;
    // The derived spec's own `references:` sit alongside inherited relations.
    table.with_spec_references(&spec.references, Some(resolver), build_mysql_table)
}

/// Apply a `rhai:` transform to a base select. Feature-gated like
/// [`table_from_rhai`].
#[cfg(feature = "rhai")]
fn eval_transform(code: &str, base: MysqlSelect, args: &[(String, String)]) -> Result<MysqlSelect> {
    crate::mysql::vista::rhai_source::eval_to_select_args(code, Some(base), args)
}

#[cfg(not(feature = "rhai"))]
fn eval_transform(
    _code: &str,
    _base: MysqlSelect,
    _args: &[(String, String)],
) -> Result<MysqlSelect> {
    Err(vantage_core::error!(
        "vista declares a `rhai:` transform but vantage-sql was built without the `rhai` feature"
    ))
}

/// The `mysql.column` block names the physical column.
pub(crate) fn build_column(
    name: &str,
    col_spec: &ColumnSpec<MysqlColumnExtras>,
) -> Result<TableColumn<AnyMysqlType>> {
    let physical = col_spec
        .driver
        .mysql
        .as_ref()
        .and_then(|b| b.column.clone());
    TableColumn::from_spec(name, col_spec, physical, column_for_type)
}

#[cfg(all(test, not(feature = "rhai")))]
mod tests {
    use sqlx::mysql::MySqlPoolOptions;

    use super::*;

    /// A pool that never connects: the specs below fail before any query.
    fn factory() -> MysqlVistaFactory {
        let pool = MySqlPoolOptions::new()
            .connect_lazy("mysql://user:pass@localhost/none")
            .unwrap();
        MysqlVistaFactory::new(MysqlDB { pool })
    }

    fn spec(yaml: &str) -> MysqlVistaSpec {
        serde_yaml_ng::from_str(yaml).unwrap()
    }

    #[tokio::test]
    async fn scripts_without_rhai_fail_the_build() {
        let with_ref_script = spec(
            r#"
name: bakery
columns:
  id: { type: int, flags: [id] }
references:
  products:
    table: product
    kind: has_many
    foreign_key: bakery_id
    mysql:
      rhai: 'table("product")'
"#,
        );
        let err = factory().build_from_spec(with_ref_script).unwrap_err();
        assert_eq!(
            err.context.get("vista").map(String::as_str),
            Some("\"bakery\"")
        );
        assert_eq!(
            err.context.get("relation").map(String::as_str),
            Some("\"products\"")
        );

        let with_modify = spec(
            r#"
name: bakery
columns:
  id: { type: int, flags: [id] }
mysql:
  modify: 'self'
"#,
        );
        let err = factory().build_from_spec(with_modify).unwrap_err();
        assert_eq!(
            err.context.get("vista").map(String::as_str),
            Some("\"bakery\"")
        );
    }
}
