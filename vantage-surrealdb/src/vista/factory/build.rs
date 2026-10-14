//! Spec → table lowering: a stored table, a `rhai:` query source, or a
//! `base:` vista derived from another spec, plus `expr:` columns.

use std::sync::Arc;

use vantage_core::Result;
#[cfg(not(feature = "rhai"))]
use vantage_core::error;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{SpecResolver, resolve_base_spec};

use super::columns::build_column;
use crate::statements::SurrealSelect;
use crate::surrealdb::SurrealDB;
use crate::vista::spec::{DriverBlockArgs as _, SurrealVistaSpec};

/// Build a `Table<SurrealDB, EmptyEntity>` from a spec. Each reference the
/// spec declares rebuilds its target through `resolver` at traversal time
/// (see `Table::with_spec_references`).
pub(super) fn build_surreal_table(
    spec: &SurrealVistaSpec,
    db: SurrealDB,
    resolver: Option<Arc<dyn SpecResolver<SurrealVistaSpec>>>,
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
        .with_spec_references(spec, resolver, build_surreal_table)?
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
    resolver: Option<Arc<dyn SpecResolver<SurrealVistaSpec>>>,
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
    base: SurrealSelect,
    args: &[(String, String)],
) -> Result<SurrealSelect> {
    crate::vista::rhai_source::eval_to_select_args(code, Some(base), args)
}

#[cfg(not(feature = "rhai"))]
fn eval_transform(
    _code: &str,
    _base: SurrealSelect,
    _args: &[(String, String)],
) -> Result<SurrealSelect> {
    Err(error!(
        "vista declares a `rhai:` transform but vantage-surrealdb was built without the `rhai` feature"
    ))
}
