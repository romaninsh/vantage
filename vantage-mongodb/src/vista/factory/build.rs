//! Spec → `Table<MongoDB, EmptyEntity>`: the collection, its pipeline view,
//! columns, references, joined (dotted) columns and contained relations.

use std::borrow::Cow;

use bson::Document;
use indexmap::IndexMap;
use vantage_core::{Context, Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::references::ContainedRelation;
use vantage_table::table::{BuildEnv, Table};
use vantage_types::EmptyEntity;
use vantage_vista::ColumnFlag;

use super::build_column;
use crate::mongodb::impls::joins::table_joins;
use crate::mongodb::{MongoDB, MongoView};
use crate::types::AnyMongoType;
use crate::vista::spec::{MongoBlock, MongoColumnBlock, MongoVistaSpec};

/// Build a `Table<MongoDB, EmptyEntity>` from a spec. A spec without columns
/// still gets an `_id` column, so the table knows its key. Each reference the
/// spec declares rebuilds its target through the resolver at traversal time
/// (see `Table::with_spec_references`). The built table goes through `env`'s
/// injector last.
pub(crate) fn build_mongo_table(
    spec: &MongoVistaSpec,
    db: MongoDB,
    env: &BuildEnv<MongoVistaSpec, MongoDB>,
) -> Result<Table<MongoDB, EmptyEntity>> {
    let spec = with_common_nested_paths(spec)?;
    let spec = spec.as_ref();
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
        table.add_spec_columns(&spec.columns, env.context(), build_column)?;
        let id = spec.resolve_id_column("_id");
        check_auto_id(spec, &id)?;
        table.set_spec_id(&id, &spec.columns)?;
    }
    let mut table = table.with_spec_references(spec, env, build_mongo_table)?;
    // A dotted column name is a join through `has_one` references, which
    // must be declared first; resolving the chains now turns a broken one
    // into a build-time error.
    for name in joined {
        table.add_imported_column(name)?;
    }
    table_joins(&table)?;
    let table = table.with_contained_specs(&spec.contained, build_column)?;
    let mut table = with_nested_path_relations(table, spec)?;
    env.inject(&spec.name, &mut table, build_mongo_table)?;
    Ok(table)
}

/// `spec` with each column's `nested_path` and `mongo: { nested_path }`
/// the same, so the BSON path code reads the block and the shared
/// resolution (`VistaSpec::contained_relations`) the column. Two different
/// paths are an error.
pub(crate) fn with_common_nested_paths(spec: &MongoVistaSpec) -> Result<Cow<'_, MongoVistaSpec>> {
    let mut spec = Cow::Borrowed(spec);
    let names: Vec<String> = spec.columns.keys().cloned().collect();
    for name in names {
        let col = &spec.columns[&name];
        let block = col
            .driver
            .mongo
            .as_ref()
            .and_then(|m| m.nested_path.as_ref());
        match (&col.nested_path, block) {
            (Some(own), Some(mongo)) if own != mongo => {
                return Err(error!(
                    "nested_path is set twice, differently",
                    table = spec.name.as_str(),
                    column = name.as_str()
                ));
            }
            (Some(own), None) => {
                let own = own.clone();
                let col = spec.to_mut().columns.get_mut(&name).expect("listed");
                col.driver
                    .mongo
                    .get_or_insert_with(MongoColumnBlock::default)
                    .nested_path = Some(own);
            }
            (None, Some(mongo)) => {
                let mongo = mongo.clone();
                spec.to_mut().columns[&name].nested_path = Some(mongo);
            }
            _ => {}
        }
    }
    Ok(spec)
}

/// A nested column's path below the table, and its leaf column, named for
/// the path's last segment.
type NestedColumn = (Vec<String>, TableColumn<AnyMongoType>);

/// `nested_path` columns are sugar for contained relations
/// (`VistaSpec::contained_relations`): the columns under one root
/// (`address.city`, `address.zip`) make a `contains_one` `address` whose
/// table declares `city` and `zip`, and a deeper path (`address.geo.lat`)
/// nests another `contains_one` (`geo`) in it. A write to them so goes
/// through that table, and merges into the stored sub-document. Reads
/// still walk each column's path.
fn with_nested_path_relations(
    table: Table<MongoDB, EmptyEntity>,
    spec: &MongoVistaSpec,
) -> Result<Table<MongoDB, EmptyEntity>> {
    let mut nested = Vec::new();
    for (root, relation) in spec.contained_relations()? {
        if spec.contained.contains_key(&root) {
            continue;
        }
        for (rest, column) in &relation.columns {
            let path: Vec<String> = std::iter::once(root.clone())
                .chain(rest.split('.').map(str::to_string))
                .collect();
            let leaf = build_column(&path[path.len() - 1], column)?;
            nested.push((path, leaf));
        }
    }
    add_nested_relations(table, nested)
}

/// The same sugar for a typed table wrapped as a vista (`from_table`): each
/// column whose alias is a dotted path (`with_alias("address.city")`). A
/// root the table already declares as a column or a contained relation is
/// left as it is.
pub(crate) fn with_alias_path_relations(
    table: Table<MongoDB, EmptyEntity>,
) -> Result<Table<MongoDB, EmptyEntity>> {
    let nested: Vec<NestedColumn> = table
        .columns()
        .values()
        .filter_map(|col| col.alias())
        .filter(|alias| alias.contains('.'))
        .map(|alias| alias.split('.').map(str::to_string).collect::<Vec<_>>())
        .filter(|path| {
            !table.columns().contains_key(&path[0])
                && table.contained_relation(&path[0]).is_none()
                && !table
                    .vista_contained()
                    .iter()
                    .any(|r| r.host_column.as_deref() == Some(path[0].as_str()))
        })
        .map(|path| {
            let leaf = TableColumn::<AnyMongoType>::new(path[path.len() - 1].clone());
            (path, leaf)
        })
        .collect();
    add_nested_relations(table, nested)
}

fn add_nested_relations(
    mut table: Table<MongoDB, EmptyEntity>,
    nested: Vec<NestedColumn>,
) -> Result<Table<MongoDB, EmptyEntity>> {
    let mut by_root: IndexMap<String, Vec<NestedColumn>> = IndexMap::new();
    for (path, col) in nested {
        by_root
            .entry(path[0].clone())
            .or_default()
            .push((path[1..].to_vec(), col));
    }
    for (root, members) in by_root {
        let hosted = table
            .vista_contained()
            .iter()
            .any(|r| r.host_column.as_deref() == Some(root.as_str()));
        // The root may also be a column of its own, read whole at its own
        // name: the nested columns read and write under that name.
        let stored_elsewhere = table
            .columns()
            .get(&root)
            .and_then(|col| col.alias())
            .is_some_and(|alias| alias != root);
        if table.contained_relation(&root).is_some() || hosted || stored_elsewhere {
            return Err(error!(
                "a nested_path root is also a contained relation of the table, or a column \
                 stored under another name",
                table = table.table_name(),
                root = root.as_str()
            ));
        }
        let db = table.data_source().clone();
        let mut target = Table::<MongoDB, EmptyEntity>::new(root.clone(), db);
        let mut deeper = Vec::new();
        for (rest, col) in members {
            if rest.len() == 1 {
                target.add_column(col);
            } else {
                deeper.push((rest, col));
            }
        }
        let target = add_nested_relations(target, deeper)?;
        // Sugar: it checks writes to `root`, and isn't listed as a relation.
        table = table.with_contained(
            ContainedRelation::one(root.clone(), root.clone(), move |_| target.clone()).unlisted(),
        );
    }
    Ok(table)
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

/// MongoDB generates only `_id`, so an `auto` id stored under another path
/// would be missing from every inserted row.
fn check_auto_id(spec: &MongoVistaSpec, id: &str) -> Result<()> {
    let Some(column) = spec.columns.get(id) else {
        return Ok(());
    };
    if !column.has_flag(ColumnFlag::Auto) {
        return Ok(());
    }
    let path = match column.driver.mongo.as_ref() {
        Some(block) => block.resolved_path(id)?,
        None => None,
    };
    if path.as_deref().unwrap_or(&[id.to_string()]) == ["_id"] {
        return Ok(());
    }
    Err(error!(
        "an auto id column must be stored at `_id`, the one field MongoDB generates; \
         map it there with `mongo: { field: _id }` or drop the `auto` flag",
        table = spec.name.as_str(),
        column = id
    ))
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
