//! Lowering a YAML spec onto a SQL table and its Vista metadata, the same
//! for every dialect. The source is a physical table, a `rhai:`-built
//! SELECT, or a `base:` vista's SELECT (which `rhai:` may transform); then
//! come the columns, the id, the references, dotted imports and contained
//! relations.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
#[cfg(not(feature = "rhai"))]
use vantage_core::error;
use vantage_expressions::{Expressive, SelectableDataSource};
use vantage_table::source::SelectSource;
use vantage_table::table::Table;
use vantage_table::table::spec::spec_references;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;
use vantage_vista::{
    ColumnSpec, ReferenceSpec, SpecResolver, VistaMetadata, VistaSpec, resolve_base_spec,
};

use super::{SqlShellDialect, metadata_options};

/// The spec a dialect's factory builds from.
pub(crate) type SpecOf<T> = VistaSpec<
    <T as SqlSpecDialect>::TableExtras,
    <T as SqlSpecDialect>::ColumnExtras,
    <T as SqlSpecDialect>::ReferenceExtras,
>;

/// What a spec's driver block (`sqlite:`, `postgres:`, `mysql:`) says about
/// where the table's rows come from. A spec without the block reads the
/// table its name names.
#[derive(Default)]
pub(crate) struct SourceBlock<'a> {
    pub table: Option<&'a str>,
    pub rhai: Option<&'a str>,
    pub base: Option<&'a str>,
    pub inherit_columns: &'a [String],
    pub inherit_relations: &'a [String],
    /// Observation args, the `args` map a `rhai:` script sees.
    pub args: &'a [(String, String)],
}

/// What lowering a spec asks of a dialect beyond its shell: its YAML driver
/// blocks and its Rhai query engine.
pub(crate) trait SqlSpecDialect:
    SqlShellDialect<
        Value: From<String> + Into<CborValue>,
        Source = SelectSource<<Self as SqlSpecDialect>::Query>,
    > + SelectableDataSource<
        <Self as TableSource>::Value,
        <Self as TableSource>::Condition,
        Select = <Self as SqlSpecDialect>::Query,
    >
{
    type TableExtras: 'static;
    type ColumnExtras: 'static;
    type ReferenceExtras: 'static;
    /// The dialect's SELECT.
    type Query: Expressive<<Self as TableSource>::Value> + Clone;

    fn source_block(spec: &SpecOf<Self>) -> SourceBlock<'_>;

    /// A spec column as the dialect's typed column, under the physical name
    /// its column block gives.
    fn spec_column(
        name: &str,
        spec: &ColumnSpec<Self::ColumnExtras>,
    ) -> Result<Self::Column<Self::AnyType>>;

    /// The Rhai script a reference builds its target with, for a dialect
    /// whose reference blocks declare one.
    fn reference_script(_reference: &ReferenceSpec<Self::ReferenceExtras>) -> Option<String> {
        None
    }

    /// Run a `rhai:` script into a SELECT, `base` in scope when given.
    #[cfg(feature = "rhai")]
    fn eval_select(
        code: &str,
        base: Option<Self::Query>,
        args: &[(String, String)],
    ) -> Result<Self::Query>;
}

/// A spec lowered for a dialect's shell.
pub(crate) struct LoweredSpec<T: SqlSpecDialect> {
    pub table: Table<T, EmptyEntity>,
    pub metadata: VistaMetadata,
    /// A `rhai:` or `base:` source is a query, which takes no writes.
    pub read_only: bool,
}

/// Build `spec`'s table and describe it to Vista. Each spec reference, a
/// `references:` entry or a column-level `references:`, is lowered from the
/// spec itself, so its target is the spec name the resolver knows rather
/// than whatever table that spec reads; relations a derived table inherits
/// keep their built table's name.
pub(crate) fn lower_spec<T>(
    spec: &SpecOf<T>,
    db: T,
    resolver: Option<Arc<dyn SpecResolver<SpecOf<T>>>>,
) -> Result<LoweredSpec<T>>
where
    T: SqlSpecDialect,
    T::Column<T::AnyType>: Expressive<T::Value>,
{
    #[cfg(not(feature = "rhai"))]
    if let Some((relation, _)) = spec
        .references
        .iter()
        .find(|(_, r)| T::reference_script(r).is_some())
    {
        return Err(error!(
            "reference declares a `rhai:` build script but vantage-sql was built without the `rhai` feature",
            vista = spec.name.as_str(),
            relation = relation.as_str()
        ));
    }

    let table = build_spec_table(spec, db, resolver)?;
    let mut metadata = table.vista_metadata(metadata_options());
    for mut lowered in spec_references(spec, &table.id_field_name())? {
        if let Some(code) = spec
            .references
            .get(&lowered.name)
            .and_then(T::reference_script)
        {
            lowered = lowered.with_build_script(code)?;
        }
        metadata = metadata.with_reference(lowered);
    }
    let block = T::source_block(spec);
    let read_only = block.rhai.is_some() || block.base.is_some();
    Ok(LoweredSpec {
        table,
        metadata,
        read_only,
    })
}

/// Build the table `spec` describes. Each `references:` entry rebuilds its
/// target through `resolver` at traversal time (see
/// `Table::with_spec_references`).
pub(crate) fn build_spec_table<T>(
    spec: &SpecOf<T>,
    db: T,
    resolver: Option<Arc<dyn SpecResolver<SpecOf<T>>>>,
) -> Result<Table<T, EmptyEntity>>
where
    T: SqlSpecDialect,
    T::Column<T::AnyType>: Expressive<T::Value>,
{
    let block = T::source_block(spec);
    if let Some(base) = block.base {
        return build_derived_table(spec, &block, base, db, resolver);
    }

    let mut table = match block.rhai {
        Some(code) => {
            let select = rhai_select::<T>(code, None, block.args)?;
            Table::from_select(db, spec.name.clone(), select)
        }
        None => Table::new(block.table.unwrap_or(&spec.name), db),
    };

    // Dotted spec columns (`author.name`) are implicit-reference imports, not
    // plain columns: a raw `add_column` would select a nonexistent
    // identifier. They lower through `with_active_columns` below, once the
    // references they traverse are registered.
    for (name, col_spec) in spec.columns.iter().filter(|(n, _)| !n.contains('.')) {
        table.add_spec_column(name, col_spec, T::spec_column)?;
    }
    table.set_spec_id_field(&spec.resolve_id_column("id"))?;

    let mut table = table.with_spec_references(spec, resolver, build_spec_table::<T>)?;

    if spec.columns.keys().any(|n| n.contains('.')) {
        // Every non-lazy spec column is listed, so the plain set keeps
        // projecting while each dotted name becomes a read-only correlated
        // import aliased under the literal dotted name.
        let names: Vec<&str> = spec
            .columns
            .iter()
            .filter(|(_, c)| c.lazy.is_none())
            .map(|(n, _)| n.as_str())
            .collect();
        table = table.with_active_columns(&names)?;
    }

    table.with_contained_specs(&spec.contained, T::spec_column)
}

/// Build a derived table: resolve `base` eagerly, build its table, run the
/// optional `rhai:` transform over its `select()` (with `base` in the
/// script's scope), inherit the listed columns and relations, and add the
/// spec's own references alongside the inherited ones.
fn build_derived_table<T>(
    spec: &SpecOf<T>,
    block: &SourceBlock<'_>,
    base: &str,
    db: T,
    resolver: Option<Arc<dyn SpecResolver<SpecOf<T>>>>,
) -> Result<Table<T, EmptyEntity>>
where
    T: SqlSpecDialect,
    T::Column<T::AnyType>: Expressive<T::Value>,
{
    let (resolver, base_spec) = resolve_base_spec(resolver, base)?;
    let base_table = build_spec_table(&base_spec, db, Some(resolver.clone()))?;

    let select = match block.rhai {
        Some(code) => rhai_select::<T>(code, Some(base_table.select()), block.args)?,
        None => base_table.select(),
    };
    let table = Table::derive_from_spec(
        &base_table,
        spec,
        select,
        block.inherit_columns,
        block.inherit_relations,
        T::spec_column,
    )?;
    table.with_spec_references(spec, Some(resolver), build_spec_table::<T>)
}

#[cfg(feature = "rhai")]
fn rhai_select<T: SqlSpecDialect>(
    code: &str,
    base: Option<T::Query>,
    args: &[(String, String)],
) -> Result<T::Query> {
    T::eval_select(code, base, args)
}

#[cfg(not(feature = "rhai"))]
fn rhai_select<T: SqlSpecDialect>(
    _code: &str,
    base: Option<T::Query>,
    _args: &[(String, String)],
) -> Result<T::Query> {
    Err(match base {
        None => error!(
            "vista declares a `rhai:` source but vantage-sql was built without the `rhai` feature"
        ),
        Some(_) => error!(
            "vista declares a `rhai:` transform but vantage-sql was built without the `rhai` feature"
        ),
    })
}
