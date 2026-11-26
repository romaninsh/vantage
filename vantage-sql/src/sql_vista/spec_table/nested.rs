//! `nested_path` columns on SQL: `label` at `name.en_US` reads key `en_US`
//! of the JSON column `name` through the dialect's JSON extraction, and is
//! stored at `name.en_US`, so a write merges that key into the host's
//! `contains_one` (see `VistaSpec::contained_relations`).

use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::references::ContainedRelation;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike as _;
use vantage_types::EmptyEntity;
use vantage_vista::{ColumnFlag, ColumnSpec, ContainedYaml};

use super::{SpecOf, SqlSpecDialect};

/// Each nested column's host and key. A path is one key below its host,
/// the key being letters, digits, `_` and `-`, as it goes into SQL as a
/// literal.
fn nested_keys<T: SqlSpecDialect>(spec: &SpecOf<T>) -> Result<Vec<(&str, &str, &str)>> {
    let mut keys = Vec::new();
    for (name, column) in &spec.columns {
        let Some(path) = &column.nested_path else {
            continue;
        };
        let key_ok = |key: &str| {
            !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        };
        match path.split_once('.') {
            Some((host, key)) if key_ok(key) => keys.push((name.as_str(), host, key)),
            _ => {
                return Err(error!(
                    "nested_path names a JSON column and one key in it (`name.en_US`), the key \
                     being letters, digits, `_` and `-`",
                    column = name,
                    nested_path = path
                ));
            }
        }
    }
    Ok(keys)
}

/// A `base:` spec reads the base's select and is read-only, so a nested
/// column belongs on the base.
pub(super) fn refuse_on_derived<T: SqlSpecDialect>(spec: &SpecOf<T>) -> Result<()> {
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
/// declare is a hidden `json` column, and each nested column reads its key.
/// A nested column is read and written at its path, and its host at its own
/// name, so a column stored elsewhere (`sqlite: { column: … }`) is refused.
pub(super) fn with_nested_columns<T>(
    mut table: Table<T, EmptyEntity>,
    spec: &SpecOf<T>,
) -> Result<Table<T, EmptyEntity>>
where
    T: SqlSpecDialect,
    T::ColumnExtras: Default,
{
    for (name, host, key) in nested_keys::<T>(spec)? {
        let path = format!("{host}.{key}");
        let stored = |column: &str| table.columns().get(column).and_then(|c| c.alias());
        if stored(name).is_some_and(|at| at != path) {
            return Err(error!(
                "a nested_path column is stored at its path, not under another column name",
                column = name,
                nested_path = path
            ));
        }
        if stored(host).is_some_and(|at| at != host) {
            return Err(error!(
                "a nested_path host is read at its own name, not under another column name",
                column = name,
                host = host
            ));
        }
        if !table.columns().contains_key(host) {
            let hidden = ColumnSpec::<T::ColumnExtras>::new()
                .with_type("json")
                .with_flag(ColumnFlag::Hidden);
            table.add_spec_column(host, &hidden, T::spec_column)?;
        }
        let read = T::nested_read(host, key);
        table = table.with_expression(name, move |_| read.clone());
    }
    Ok(table)
}

/// `table` with the contained relations `nested_path` makes, which
/// `contained_relations` lists after the spec's own `contained:`. Unlisted:
/// they check writes to their host, and aren't relations of the vista.
pub(super) fn with_nested_relations<T>(
    mut table: Table<T, EmptyEntity>,
    spec: &SpecOf<T>,
    relations: &IndexMap<String, ContainedYaml<T::ColumnExtras>>,
) -> Result<Table<T, EmptyEntity>>
where
    T: SqlSpecDialect,
    T::Column<T::AnyType>: Clone,
{
    for (name, relation) in relations {
        if spec.contained.contains_key(name) {
            continue;
        }
        let columns = relation
            .columns
            .iter()
            .map(|(key, column)| T::spec_column(key, column))
            .collect::<Result<Vec<_>>>()?;
        let target = name.clone();
        table = table.with_contained(
            ContainedRelation::one(name, &relation.host_column, move |db| {
                let mut record = Table::<T, EmptyEntity>::new(target.clone(), db);
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
