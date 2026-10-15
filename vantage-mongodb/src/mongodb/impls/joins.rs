//! A table's imported (dotted) columns as `$lookup` join chains.
//!
//! vantage-table validates and registers an imported column
//! (`Table::add_imported_column`); its hop chain is resolved here from the
//! table's references whenever a read is built, so the Rust API and the YAML
//! factory share one lowering. A column's BSON path rides on its alias, so
//! nested foreign keys and target fields resolve to their stored paths.
//!
//! Each hop is a `$lookup` with `localField`/`foreignField`, never a
//! correlated sub-pipeline. Correlated subqueries are not implemented:
//! `Table::get_subquery_*` panics on MongoDB, as `related_correlated_condition`
//! is the trait's `unimplemented!` default.
//!
//! A typed table's raw conditions address stored BSON paths: a hand-written
//! `doc! { "client.name": .. }` reads a nested `client.name` field, not the
//! join. To filter or order by a joined column, wrap the table as a Vista
//! (`MongoVistaFactory::from_table`) and use its condition and order API,
//! which addresses joined columns by their dotted names.

use std::collections::HashMap;

use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Entity};

use crate::mongodb::MongoDB;
use crate::select::{JoinColumn, JoinHop, join_alias};
use crate::types::AnyMongoType;

/// The table's imported columns in column order; a column's position is its
/// join index.
pub(crate) fn imported_columns<E: Entity<AnyMongoType>>(table: &Table<MongoDB, E>) -> Vec<&str> {
    table
        .columns()
        .keys()
        .filter(|name| table.is_imported_column(name))
        .map(String::as_str)
        .collect()
}

/// The field an imported column is computed into, `None` for any other name.
pub(crate) fn join_alias_of<E: Entity<AnyMongoType>>(
    table: &Table<MongoDB, E>,
    name: &str,
) -> Option<String> {
    imported_columns(table)
        .iter()
        .position(|n| *n == name)
        .map(join_alias)
}

/// Resolve every imported column of `table` into its join chain. Each
/// relation prefix (`client`, `client.country`) is resolved once, however
/// many columns traverse it.
pub(crate) fn table_joins<E: Entity<AnyMongoType>>(
    table: &Table<MongoDB, E>,
) -> Result<Vec<JoinColumn>> {
    let names = imported_columns(table);
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let mut resolver = Resolver {
        root: table.clone().into_entity::<EmptyEntity>(),
        hops: HashMap::new(),
    };
    names.iter().map(|name| resolver.resolve(name)).collect()
}

struct Resolver {
    root: Table<MongoDB, EmptyEntity>,
    /// Relation prefix → the hop reaching it and the target it reaches.
    hops: HashMap<String, (JoinHop, Table<MongoDB, EmptyEntity>)>,
}

impl Resolver {
    fn resolve(&mut self, name: &str) -> Result<JoinColumn> {
        let parts: Vec<&str> = name.split('.').collect();
        let Some((field, relations)) = parts.split_last().filter(|(_, r)| !r.is_empty()) else {
            return Err(error!("joined column name must be dotted", column = name));
        };
        let mut hops = Vec::with_capacity(relations.len());
        for depth in 1..=relations.len() {
            let prefix = relations[..depth].join(".");
            if !self.hops.contains_key(&prefix) {
                let parent = match depth {
                    1 => &self.root,
                    _ => &self.hops[&relations[..depth - 1].join(".")].1,
                };
                let resolved = hop(parent, relations[depth - 1], name)?;
                self.hops.insert(prefix.clone(), resolved);
            }
            hops.push(self.hops[&prefix].0.clone());
        }
        let target = &self.hops[&relations.join(".")].1;
        Ok(JoinColumn {
            name: name.to_string(),
            hops,
            field: bson_path(target, field),
        })
    }
}

/// The hop through `relation` on `table`, and the table it reaches.
fn hop(
    table: &Table<MongoDB, EmptyEntity>,
    relation: &str,
    name: &str,
) -> Result<(JoinHop, Table<MongoDB, EmptyEntity>)> {
    let fk = table.ref_foreign_key(relation)?;
    let target = table.get_ref_target_erased(relation)?;
    if target.data_source().view().is_some() {
        return Err(error!(
            "joined column cannot traverse into a pipeline view",
            column = name,
            relation = relation
        ));
    }
    // A target that names no id column is keyed by MongoDB's own `_id`.
    let foreign_field = match target.id_field() {
        Some(_) => bson_path(&target, &target.id_field_name()),
        None => "_id".to_string(),
    };
    let hop = JoinHop {
        from: target.table_name().to_string(),
        local_field: bson_path(table, &fk),
        foreign_field,
    };
    Ok((hop, target))
}

fn bson_path(table: &Table<MongoDB, EmptyEntity>, column: &str) -> String {
    table
        .columns()
        .get(column)
        .and_then(|c| c.alias())
        .unwrap_or(column)
        .to_string()
}
