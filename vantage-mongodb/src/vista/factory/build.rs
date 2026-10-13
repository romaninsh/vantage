//! Spec → `Table<MongoDB, EmptyEntity>`: the collection, its pipeline view,
//! columns, references, joined (dotted) columns and contained relations.

use bson::Document;
use vantage_core::{Context, Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::SpecResolver;

use super::build_column;
use crate::mongodb::impls::joins::table_joins;
use crate::mongodb::{MongoDB, MongoView};
use crate::types::AnyMongoType;
use crate::vista::spec::{MongoBlock, MongoVistaSpec};

/// Build a `Table<MongoDB, EmptyEntity>` from a spec. A spec without columns
/// still gets an `_id` column, so the table knows its key. Each `references:`
/// entry rebuilds its target through `resolver` at traversal time (see
/// `Table::with_spec_references`).
pub(crate) fn build_mongo_table(
    spec: &MongoVistaSpec,
    db: MongoDB,
    resolver: Option<SpecResolver<MongoVistaSpec>>,
) -> Result<Table<MongoDB, EmptyEntity>> {
    let block = spec.driver.mongo.as_ref();
    let collection = block
        .and_then(|m| m.collection.clone())
        .unwrap_or_else(|| spec.name.clone());
    // The view comes from this spec alone: a reference target built over a
    // view-carrying source reads its own collection.
    let stages = block
        .map(view_stages)
        .transpose()
        .with_context(|| error!("pipeline view can't be built", table = spec.name.as_str()))?
        .flatten();
    let db = match stages {
        Some(stages) => db.with_view(MongoView::new(collection.clone(), stages)),
        None => db.without_view(),
    }
    .with_server_view(block.is_some_and(|m| m.view));

    let joined: Vec<&str> = spec
        .columns
        .keys()
        .filter(|name| name.contains('.'))
        .map(String::as_str)
        .collect();
    check_joined_columns(spec, &joined, db.view().is_some())?;

    let mut table = Table::<MongoDB, EmptyEntity>::new(collection, db);
    if spec.columns.is_empty() {
        table.add_column(TableColumn::<AnyMongoType>::new("_id"));
        table.set_spec_id_field("_id")?;
    } else {
        table.add_spec_columns(&spec.columns, build_column)?;
        table.set_spec_id_field(&spec.resolve_id_column("_id"))?;
    }
    let mut table = table.with_spec_references(&spec.references, resolver, build_mongo_table)?;
    // A dotted column name is a join through `has_one` references, which
    // must be declared first; resolving the chains now turns a broken one
    // into a build-time error.
    for name in joined {
        table.add_imported_column(name)?;
    }
    table_joins(&table)?;
    table.with_contained_specs(&spec.contained, build_column)
}

/// The pipeline a `mongo:` block declares, from `pipeline:` or a `rhai:`
/// script; `Ok(None)` without either.
fn view_stages(block: &MongoBlock) -> Result<Option<Vec<Document>>> {
    match &block.rhai {
        Some(_) if block.pipeline.is_some() => Err(error!(
            "a mongo block takes either `pipeline:` or `rhai:`, not both"
        )),
        Some(code) => pipeline_from_rhai(code, &block.args).map(Some),
        None => block.pipeline_stages(),
    }
}

#[cfg(feature = "rhai")]
fn pipeline_from_rhai(code: &str, args: &[(String, String)]) -> Result<Vec<Document>> {
    crate::vista::scripting::eval_pipeline(code, args)
}

#[cfg(not(feature = "rhai"))]
fn pipeline_from_rhai(_code: &str, _args: &[(String, String)]) -> Result<Vec<Document>> {
    Err(missing_rhai("rhai"))
}

/// A spec declares a script (`mongo.<slot>:`) the build cannot run.
#[cfg_attr(feature = "rhai", allow(dead_code))]
pub(crate) fn missing_rhai(slot: &str) -> vantage_core::VantageError {
    error!(
        "vista declares a Rhai script but vantage-mongodb was built without the `rhai` feature",
        slot = slot
    )
}

/// A dotted column names a join, so it cannot carry a BSON path of its own,
/// and a pipeline view reads computed rows that cannot be joined.
fn check_joined_columns(spec: &MongoVistaSpec, joined: &[&str], is_view: bool) -> Result<()> {
    let Some(first) = joined.first() else {
        return Ok(());
    };
    if is_view {
        return Err(error!(
            "a pipeline view cannot declare a joined (dotted) column",
            table = spec.name.as_str(),
            column = *first
        ));
    }
    for name in joined {
        if spec.columns[*name].driver.mongo.is_some() {
            return Err(error!(
                "a joined (dotted) column cannot set a mongo path",
                table = spec.name.as_str(),
                column = *name
            ));
        }
    }
    Ok(())
}
