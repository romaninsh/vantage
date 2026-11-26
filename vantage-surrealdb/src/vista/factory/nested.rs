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

/// `table` with the spec's nested columns: each host the spec doesn't
/// declare is a hidden `json` column, and each nested column reads its
/// field. A path is one field below its host.
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
