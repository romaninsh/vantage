//! Reference keys must be stored under their own field name.
//!
//! Traversal narrows the target by its key column's *spec name*
//! (`HasOne`/`HasMany` in vantage-table call `eq_value_condition` and add an
//! invariant without any column-path context), so a key stored under another
//! field (`field:`) or at a nested path (`nested_path:`) would match nothing
//! and write a stray field. Both factory entry points refuse such a reference:
//! `table_from_spec` from the spec, and every wrap of a table (traversal
//! targets included) from its columns. Joined (dotted) columns resolve their
//! own paths and are unaffected.
//!
//! The keys checked are the columns traversal touches: each join key's
//! parent column and the target column it constrains. For a single-key join
//! that is a `has_one`'s foreign key and the target's id column, or a
//! `has_many`'s source id column and the foreign key on the target; a spec's
//! `keys:` list checks every pair. A spec reference carrying a
//! `mongo: { rhai: }` build script is exempt when the `rhai` feature is on:
//! its traversal runs the script, not the foreign-key condition.

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_table::table::spec::spec_references;
use vantage_types::EmptyEntity;
use vantage_vista::{ReferenceKind, SpecResolver};

use crate::mongodb::MongoDB;
use crate::vista::spec::MongoVistaSpec;

/// Check every join `spec` declares, `references:` entries and column-level
/// `references:` alike. A target the resolver doesn't know is a bare
/// collection keyed by `_id`, stored as `_id`.
pub(super) fn check_spec(
    spec: &MongoVistaSpec,
    resolver: Option<&Arc<dyn SpecResolver<MongoVistaSpec>>>,
) -> Result<()> {
    for join in spec_references(spec, &spec.resolve_id_column("_id"))? {
        let relation = join.name.as_str();
        let scripted = spec
            .references
            .get(relation)
            .and_then(|reference| reference.driver.mongo.as_ref())
            .is_some_and(|block| block.rhai.is_some());
        if scripted && cfg!(feature = "rhai") {
            continue;
        }
        let target = resolver.and_then(|resolver| resolver.resolve(&join.target));
        for key in &join.keys {
            refuse_renamed(relation, &key.from, spec_path_is_renamed(spec, &key.from)?)?;
            let Some(target) = &target else { continue };
            let column = key
                .to
                .clone()
                .unwrap_or_else(|| target.resolve_id_column("_id"));
            refuse_renamed(relation, &column, spec_path_is_renamed(target, &column)?)?;
        }
    }
    Ok(())
}

/// Check every reference registered on a typed table, building each target
/// to read its columns. A column's BSON path rides on its alias. A wrapped
/// table runs no reference scripts, so no reference is exempt here.
pub(super) fn check_table(table: &Table<MongoDB, EmptyEntity>) -> Result<()> {
    for (relation, kind) in table.ref_kinds() {
        let fk = table.ref_foreign_key(&relation)?;
        let target = table.get_ref_target_erased(&relation)?;
        let (own, on_target) = match kind {
            ReferenceKind::HasOne => (fk, target.id_field_name()),
            ReferenceKind::HasMany => (table.id_field_name(), fk),
            // Contained relations are registered apart and carry no join keys.
            _ => continue,
        };
        refuse_renamed(&relation, &own, alias_is_renamed(table, &own))?;
        refuse_renamed(&relation, &on_target, alias_is_renamed(&target, &on_target))?;
    }
    Ok(())
}

fn refuse_renamed(relation: &str, column: &str, renamed: bool) -> Result<()> {
    if renamed {
        return Err(error!(
            "reference key must be stored under its own field name",
            relation = relation,
            column = column
        ));
    }
    Ok(())
}

/// Whether the spec stores `column` anywhere but a field of its own name.
fn spec_path_is_renamed(spec: &MongoVistaSpec, column: &str) -> Result<bool> {
    let block = spec
        .columns
        .get(column)
        .and_then(|c| c.driver.mongo.as_ref());
    Ok(match block {
        Some(block) => block
            .resolved_path(column)?
            .is_some_and(|path| path != [column]),
        None => false,
    })
}

/// Whether the table's `column` carries an alias other than its own name.
fn alias_is_renamed(table: &Table<MongoDB, EmptyEntity>, column: &str) -> bool {
    table
        .columns()
        .get(column)
        .and_then(|c| c.alias())
        .is_some_and(|alias| alias != column)
}
