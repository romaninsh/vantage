//! `nested_path` columns: `label` at `name.en_US` reads field `en_US` of
//! the object in `name`, and is stored at `name.en_US`, so a write merges
//! that field into the host's `contains_one` (see
//! `VistaSpec::contained_relations`).

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::references::ContainedRelation;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::{ColumnFlag, ColumnSpec, ContainedYaml};

use super::columns::build_column;
use crate::identifier::Identifier;
use crate::surrealdb::SurrealDB;
use crate::vista::spec::{SurrealColumnExtras, SurrealVistaSpec};

/// A `base:` spec reads the base's select, so a nested column belongs on
/// the base.
pub(super) fn refuse_on_derived(spec: &SurrealVistaSpec) -> Result<()> {
    match spec.columns.iter().find(|(_, c)| c.nested_path.is_some()) {
        Some((name, _)) => Err(error!(
            "nested_path isn't supported on a `base:` vista; declare the column on its base",
            vista = spec.name.as_str(),
            column = name
        )),
        None => Ok(()),
    }
}

/// `table` with the spec's nested columns: each host the spec doesn't
/// declare is a hidden `json` column, and each nested column reads its
/// field. A path is one field below its host. A nested column is read and
/// written at its path, and its host at its own name, so a column stored
/// elsewhere (`surreal: { field: … }`) is refused.
pub(super) fn with_nested_columns(
    mut table: Table<SurrealDB, EmptyEntity>,
    spec: &SurrealVistaSpec,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    for (name, column) in &spec.columns {
        let Some(path) = &column.nested_path else {
            continue;
        };
        let Some((host, key)) = path.split_once('.').filter(|(_, key)| !key.contains('.')) else {
            return Err(error!(
                "nested_path names an object field and one key in it (`name.en_US`)",
                column = name,
                nested_path = path
            ));
        };
        let stored = |column: &str| table.columns().get(column).and_then(|c| c.alias());
        if stored(name).is_some_and(|at| at != path.as_str()) {
            return Err(error!(
                "a nested_path column is stored at its path, not under another field",
                column = name,
                nested_path = path
            ));
        }
        if stored(host).is_some_and(|at| at != host) {
            return Err(error!(
                "a nested_path host is read at its own name, not under another field",
                column = name,
                host = host
            ));
        }
        if !table.columns().contains_key(host) {
            let hidden = ColumnSpec::<SurrealColumnExtras>::new()
                .with_type("json")
                .with_flag(ColumnFlag::Hidden);
            table.add_spec_column(host, &hidden, build_column)?;
        }
        let read = Identifier::new(host).dot(key);
        table = table.with_expression(name, move |_| read.clone());
    }
    Ok(table)
}

/// `table` with the contained relations `nested_path` makes, unlisted: they
/// check writes to their host, and aren't relations of the vista.
pub(super) fn with_nested_relations(
    mut table: Table<SurrealDB, EmptyEntity>,
    spec: &SurrealVistaSpec,
    relations: &IndexMap<String, ContainedYaml<SurrealColumnExtras>>,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    for (name, relation) in relations {
        if spec.contained.contains_key(name) {
            continue;
        }
        let columns = relation
            .columns
            .iter()
            .map(|(key, column)| build_column(key, column))
            .collect::<Result<Vec<_>>>()?;
        let target = name.clone();
        table = table.with_contained(
            ContainedRelation::one(name, &relation.host_column, move |db| {
                let mut record = Table::<SurrealDB, EmptyEntity>::new(target.clone(), db);
                for column in &columns {
                    record.add_column(column.clone());
                }
                record
            })
            .unlisted(),
        );
    }
    Ok(table)
}
