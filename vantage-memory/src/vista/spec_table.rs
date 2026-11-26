//! Lowering a YAML spec onto a typed memory table, its store table and its
//! Vista metadata.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::spec::spec_references;
use vantage_table::table::{BuildEnv, BuildKind, Table};
use vantage_types::EmptyEntity;
use vantage_vista::{Column, ColumnFlag, ColumnSpec, NoExtras, VistaMetadata};

use super::MemoryVistaSpec;
use super::affinity::{Affinity, conform_record};
use crate::{AnyMemoryType, MemoryDB, TableDef, seed};

/// The id column a spec gets when it names none.
const DEFAULT_ID_COLUMN: &str = "id";

/// The table `spec` describes, run through `env`'s injector. A root build
/// defines its store table first, with the spec's id column and indexes,
/// and seeds it when empty. Each reference rebuilds its target through `env`'s resolver when
/// traversed.
pub(super) fn build_memory_table(
    spec: &MemoryVistaSpec,
    db: MemoryDB,
    env: &BuildEnv<MemoryVistaSpec, MemoryDB>,
) -> Result<Table<MemoryDB, EmptyEntity>> {
    if !spec.contained.is_empty() {
        return Err(error!(
            "Contained relations are not supported by vantage-memory",
            vista = spec.name
        ));
    }
    if let Some(name) = spec
        .columns
        .iter()
        .find_map(|(name, c)| c.nested_path.as_ref().map(|_| name))
    {
        return Err(error!(
            format!("column {name}: nested_path isn't supported by vantage-memory"),
            vista = spec.name
        ));
    }
    let metadata = metadata_from_spec(spec)?;
    let id_column = spec.resolve_id_column(DEFAULT_ID_COLUMN);
    // A traversal must not reseed a table emptied since, nor seed past an
    // open transaction's rollback.
    if env.kind == BuildKind::Root {
        define_store_table(spec, &db, &metadata, &id_column)?;
    }
    let mut table = Table::<MemoryDB, EmptyEntity>::new(spec.name.clone(), db);
    table.add_spec_columns(&spec.columns, build_column)?;
    // An undeclared id column is the store table's, which `MemoryDB` reads.
    if table.columns().contains_key(&id_column) {
        table.set_spec_id(&id_column, &spec.columns)?;
    }
    let mut table = table.with_spec_references(spec, env, build_memory_table)?;
    env.inject(&spec.name, &mut table, build_memory_table)?;
    Ok(table)
}

/// The store table may predate the spec, keeping its original definition.
/// Seeding only an empty table means rebuilding neither reverts edits made
/// since nor duplicates rows without an id; seeded cells take their declared
/// type's form, as written cells do.
fn define_store_table(
    spec: &MemoryVistaSpec,
    db: &MemoryDB,
    metadata: &VistaMetadata,
    id_column: &str,
) -> Result<()> {
    let block = &spec.driver.memory;
    let table = db.store().define(
        &spec.name,
        TableDef {
            id_column: id_column.to_string(),
            indexed: block.indexed.clone(),
            id_prefix: None,
        },
    );
    if table.id_column() != id_column {
        return Err(error!(
            "Table already exists with another id column",
            table = spec.name,
            existing = table.id_column(),
            requested = id_column
        ));
    }
    for column in &block.indexed {
        table.add_index(column);
    }
    if let Some(path) = &block.seed
        && table.is_empty()
    {
        seed::load_file_with(&table, path, |row| conform_record(metadata, id_column, row))?;
    }
    Ok(())
}

/// A column without `type:` is `AnyMemoryType`, which reads back as
/// untyped; an integer or text type is `i64` or `String`; any other type
/// has no stored form.
fn build_column(name: &str, spec: &ColumnSpec<NoExtras>) -> Result<TableColumn<AnyMemoryType>> {
    TableColumn::from_spec(name, spec, None, |name, ty| {
        Ok(match (&spec.col_type, Affinity::of_type(Some(ty))) {
            (None, _) => TableColumn::new(name),
            (_, Affinity::Integer) => TableColumn::from_column(TableColumn::<i64>::new(name)),
            (_, Affinity::Text) => TableColumn::from_column(TableColumn::<String>::new(name)),
            _ => TableColumn::from_column(TableColumn::<CborValue>::new(name)),
        })
    })
}

/// Every stored column is orderable: the store sorts on any field. A column
/// without `type:` is untyped, so its cells are stored as given. A `lazy:`
/// column is computed by the Vista. An `expr:` column is refused.
pub(super) fn metadata_from_spec(spec: &MemoryVistaSpec) -> Result<VistaMetadata> {
    let id = spec.resolve_id_column(DEFAULT_ID_COLUMN);
    let mut metadata = VistaMetadata::new().with_id_column(id.clone());
    for (name, col) in &spec.columns {
        if col.expr.is_some() {
            return Err(error!(
                "Server-side `expr:` columns are not supported by vantage-memory",
                column = name
            ));
        }
        let column = match col.lazy_column(name)? {
            Some(computed) => computed,
            None => {
                let column = match &col.col_type {
                    Some(ty) => Column::new(name, ty),
                    None => Column::untyped(name),
                };
                col.flags
                    .iter()
                    .cloned()
                    .chain([ColumnFlag::Orderable])
                    .fold(column, Column::with_flag)
            }
        };
        metadata = metadata.with_column(column);
    }
    for reference in spec_references(spec, &id)? {
        metadata = metadata.with_reference(reference);
    }
    Ok(metadata)
}
