//! `SurrealVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. SurrealDB advertises reads, writes, order,
//! search, operator filters, native conditions, windowed reads, traversal
//! and (for real tables) LIVE subscriptions; a query-sourced vista is
//! read-only.

#[cfg(feature = "rhai")]
mod scripting;
#[cfg(test)]
mod tests;

use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{Orderable, Table, VistaMetadataOptions};
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{
    Capability, ReferenceSpec, SpecResolver, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
    resolve_base_spec,
};

use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::AnySurrealType;
use crate::vista::source::SurrealTableShell;
use crate::vista::spec::{
    DriverBlockArgs as _, SurrealColumnExtras, SurrealReferenceExtras, SurrealTableExtras,
    SurrealVistaSpec,
};

#[derive(Clone)]
pub struct SurrealVistaFactory {
    db: SurrealDB,
    /// Resolves a YAML spec by table name, so references and `base:` can
    /// rebuild their tables from the live spec.
    resolver: Option<SpecResolver<SurrealVistaSpec>>,
}

impl SurrealVistaFactory {
    pub fn new(db: SurrealDB) -> Self {
        Self { db, resolver: None }
    }

    /// Attach a spec resolver. Required for YAML-declared references to
    /// resolve their target tables — without it, traversals yield a
    /// column-less target Table and the next query fails loudly.
    pub fn with_resolver(mut self, resolver: SpecResolver<SurrealVistaSpec>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path. The original entity
    /// type is preserved so `with_expression` closures remain typecheckable.
    pub fn from_table<E>(&self, table: Table<SurrealDB, E>) -> Result<Vista>
    where
        E: Entity<AnySurrealType> + 'static,
    {
        let name = table.table_name().to_string();
        let metadata = metadata_from_table(&table);
        Ok(self.wrap(table, name, metadata, false))
    }

    /// Single source-construction site shared by `from_table` and
    /// `build_from_spec`. Keeps the capability set and `Vista::new` call in one
    /// place. A query-sourced (e.g. `rhai:` or `base:`) vista is `read_only`,
    /// which clears the write capabilities.
    fn wrap<E>(
        &self,
        table: Table<SurrealDB, E>,
        name: String,
        metadata: VistaMetadata,
        read_only: bool,
    ) -> Vista
    where
        E: Entity<AnySurrealType> + 'static,
    {
        let shell = SurrealTableShell::new(
            table,
            surreal_capabilities(read_only),
            metadata,
            self.resolver.clone(),
        );
        Vista::new(name, shell)
    }

    /// Build a `Table<SurrealDB, EmptyEntity>` from a spec.
    pub fn table_from_spec(
        &self,
        spec: &SurrealVistaSpec,
    ) -> Result<Table<SurrealDB, EmptyEntity>> {
        build_surreal_table(spec, self.db.clone(), self.resolver.clone())
    }
}

impl VistaFactory for SurrealVistaFactory {
    type TableExtras = SurrealTableExtras;
    type ColumnExtras = SurrealColumnExtras;
    type ReferenceExtras = SurrealReferenceExtras;

    fn build_from_spec(&self, spec: SurrealVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let read_only = spec
            .driver
            .surreal
            .as_ref()
            .is_some_and(|m| m.rhai.is_some() || m.base.is_some());
        let table = self.table_from_spec(&spec)?;
        let script = |r: &ReferenceSpec<SurrealReferenceExtras>| {
            r.driver.surreal.as_ref().and_then(|b| b.rhai.clone())
        };
        #[cfg(not(feature = "rhai"))]
        if let Some((relation, _)) = spec.references.iter().find(|(_, r)| script(r).is_some()) {
            return Err(error!(
                "reference declares a `rhai:` build script but vantage-surrealdb was built without the `rhai` feature",
                vista = vista_name.as_str(),
                relation = relation.as_str()
            ));
        }
        let mut metadata = metadata_from_table(&table);
        table.name_spec_targets(&mut metadata, &spec.references, script)?;
        let modify = spec.driver.surreal.as_ref().and_then(|b| b.modify.clone());
        #[cfg(not(feature = "rhai"))]
        if modify.is_some() {
            return Err(error!(
                "vista declares a `modify:` script but vantage-surrealdb was built without the `rhai` feature",
                vista = vista_name.as_str()
            ));
        }
        #[cfg_attr(not(feature = "rhai"), allow(unused_mut))]
        let mut vista = self.wrap(table, vista_name, metadata, read_only);

        // Final step: a `surreal: { modify }` script tweaks the just-built vista
        // (e.g. an expression condition YAML can't express). Runs last, so it
        // composes with `table`/`rhai`/`base`.
        #[cfg(feature = "rhai")]
        if let Some(code) = modify {
            vista = self.apply_modify(vista, &code)?;
        }

        Ok(vista)
    }
}

/// What the SurrealDB shell implements. A query-sourced (`rhai:` or `base:`)
/// vista takes no writes, so it confines none, and has no table for a LIVE
/// subscription. Native conditions narrow it all the same.
fn surreal_capabilities(read_only: bool) -> VistaCapabilities {
    let reads = [
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::NativeConditions,
        Capability::Window,
        Capability::Traverse,
    ];
    let table_only = [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Subscribe,
    ];
    let caps: VistaCapabilities = reads.into_iter().collect();
    if read_only {
        return caps;
    }
    table_only.into_iter().fold(caps, VistaCapabilities::with)
}

/// Build a `Table<SurrealDB, EmptyEntity>` from a spec. Each `references:`
/// entry rebuilds its target through `resolver` at traversal time (see
/// `Table::with_spec_references`).
pub(crate) fn build_surreal_table(
    spec: &SurrealVistaSpec,
    db: SurrealDB,
    resolver: Option<SpecResolver<SurrealVistaSpec>>,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    let block = spec.driver.surreal.as_ref();

    if let Some(base_name) = block.and_then(|m| m.base.clone()) {
        return build_derived_table(spec, &base_name, db, resolver);
    }

    let mut table = match block.and_then(|m| m.rhai.clone()) {
        Some(code) => table_from_rhai(spec, &code, db.clone())?,
        None => {
            let table_name = block
                .and_then(|m| m.table.clone())
                .unwrap_or_else(|| spec.name.clone());
            Table::<SurrealDB, EmptyEntity>::new(table_name, db.clone())
        }
    };

    for (name, col_spec) in &spec.columns {
        if table.add_spec_column(name, col_spec, build_column)?
            && let Some(code) = &col_spec.expr
        {
            table = add_expr_column(table, name, code)?;
        }
    }
    table.set_spec_id_field(&spec.resolve_id_column("id"))?;

    let table = table
        .with_spec_references(&spec.references, resolver, build_surreal_table)?
        .with_contained_specs(&spec.contained, build_column)?;

    // A dotted spec column (`batch.name`) is an implicit reference — route it
    // through `with_active_columns` so it becomes a validated traversal import.
    // Left as a literal column it would project a single-escaped ⟨batch.name⟩
    // field lookup: a dead column that renders NONE in every row. The active
    // set must then name every column, so pass the full declared set.
    let table = if spec.columns.keys().any(|n| n.contains('.')) {
        let names: Vec<String> = table.columns().keys().cloned().collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        table.with_active_columns(&name_refs)?
    } else {
        table
    };
    Ok(table)
}

/// Lower a column's `expr:` script into a server-side computed column via
/// `Table::with_expression` — the select then projects `(<expr>) AS <name>`
/// instead of the bare field.
#[cfg(feature = "rhai")]
fn add_expr_column(
    table: Table<SurrealDB, EmptyEntity>,
    name: &str,
    code: &str,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    let expr = crate::vista::rhai_source::eval_to_expr(code)?;
    Ok(table.with_expression(name, move |_| expr.clone()))
}

#[cfg(not(feature = "rhai"))]
fn add_expr_column(
    _table: Table<SurrealDB, EmptyEntity>,
    name: &str,
    _code: &str,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    Err(error!(
        "column declares an `expr:` script but vantage-surrealdb was built without the `rhai` feature",
        column = name
    ))
}

/// Build a query-sourced table from a `rhai:` script.
#[cfg(feature = "rhai")]
fn table_from_rhai(
    spec: &SurrealVistaSpec,
    code: &str,
    db: SurrealDB,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    let select =
        crate::vista::rhai_source::eval_to_select_args(code, None, &spec.driver_block_args())?;
    Ok(Table::from_select(db, spec.name.clone(), select))
}

#[cfg(not(feature = "rhai"))]
fn table_from_rhai(
    _spec: &SurrealVistaSpec,
    _code: &str,
    _db: SurrealDB,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    Err(error!(
        "vista declares a `rhai:` source but vantage-surrealdb was built without the `rhai` feature"
    ))
}

/// Build a derived table: resolve `base_name` eagerly via the resolver, build
/// the base table, optionally transform its `select()` through a `rhai:` script
/// (transform mode — `base` is seeded into the engine scope), and inherit the
/// listed columns/relations (see `Table::derive_from_spec`).
fn build_derived_table(
    spec: &SurrealVistaSpec,
    base_name: &str,
    db: SurrealDB,
    resolver: Option<SpecResolver<SurrealVistaSpec>>,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    let (resolver, base_spec) = resolve_base_spec(resolver, base_name)?;
    let base_table = build_surreal_table(&base_spec, db, Some(resolver))?;

    let block = spec.driver.surreal.as_ref();
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
    base: crate::statements::SurrealSelect,
    args: &[(String, String)],
) -> Result<crate::statements::SurrealSelect> {
    crate::vista::rhai_source::eval_to_select_args(code, Some(base), args)
}

#[cfg(not(feature = "rhai"))]
fn eval_transform(
    _code: &str,
    _base: crate::statements::SurrealSelect,
    _args: &[(String, String)],
) -> Result<crate::statements::SurrealSelect> {
    Err(error!(
        "vista declares a `rhai:` transform but vantage-surrealdb was built without the `rhai` feature"
    ))
}

/// The `surreal.field` block names the stored field.
pub(crate) fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<SurrealColumnExtras>,
) -> Result<TableColumn<AnySurrealType>> {
    let field = col_spec
        .driver
        .surreal
        .as_ref()
        .and_then(|b| b.field.clone());
    TableColumn::from_spec(name, col_spec, field, column_for_type)
}

/// YAML type alias → typed `Column` (then erased to `Column<AnySurrealType>`).
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnySurrealType>> {
    let col: TableColumn<AnySurrealType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "thing" | "record" | "record_id" => {
            TableColumn::from_column(TableColumn::<Thing>::new(name))
        }
        "datetime" => {
            TableColumn::from_column(TableColumn::<chrono::DateTime<chrono::Utc>>::new(name))
        }
        #[cfg(feature = "decimal")]
        "decimal" | "numeric" => {
            TableColumn::from_column(TableColumn::<rust_decimal::Decimal>::new(name))
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
    T: vantage_table::traits::table_source::TableSource + 'static,
    E: Entity<T::Value> + 'static,
    T::Column<T::AnyType>: ColumnLike<T::AnyType>,
{
    // SurrealDB sorts on any physical field. An imported implicit-reference
    // column exists only as a projection alias — ordering by its dotted name
    // would address a nonexistent field and silently not sort. expr:/lazy
    // computed columns stay orderable (they order by their projected alias);
    // all computed columns are read-only for consumers.
    // Join targets carry their built table's name; a spec entry point
    // renames them to spec names (`name_spec_targets`).
    table.vista_metadata(VistaMetadataOptions {
        orderable: Orderable::ExceptImported,
        calculated: true,
        column_flags: true,
        references: true,
        contained: true,
        ..VistaMetadataOptions::default()
    })
}
