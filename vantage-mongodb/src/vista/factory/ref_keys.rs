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
//! The keys checked per kind are the two columns traversal touches: a
//! `has_one`'s foreign key and the target's id column; a `has_many`'s source
//! id column and the foreign key on the target. A spec reference carrying a
//! `mongo: { rhai: }` build script is exempt when the `rhai` feature is on:
//! its traversal runs the script, not the foreign-key condition.

use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;
use vantage_vista::ReferenceKind;

use super::MongoSpecResolver;
use crate::mongodb::MongoDB;
use crate::vista::spec::MongoVistaSpec;

/// Check every `references:` entry of `spec`. A target the resolver doesn't
/// know is a bare collection keyed by `_id`, stored as `_id`.
pub(super) fn check_spec(
    spec: &MongoVistaSpec,
    resolver: Option<&MongoSpecResolver>,
) -> Result<()> {
    for (relation, reference) in &spec.references {
        let scripted = reference
            .driver
            .mongo
            .as_ref()
            .is_some_and(|block| block.rhai.is_some());
        if scripted && cfg!(feature = "rhai") {
            continue;
        }
        let fk = reference.foreign_key_or(relation);
        let target = resolver.and_then(|resolve| resolve(&reference.table));
        let (own, on_target) = match reference.kind {
            ReferenceKind::HasOne => (fk, target.as_ref().map(|t| t.resolve_id_column_or("_id"))),
            ReferenceKind::HasMany => (spec.resolve_id_column_or("_id"), Some(fk)),
            // A contained kind has no join keys; the table build refuses it
            // under `references:`.
            _ => continue,
        };
        refuse_renamed(relation, &own, spec_path_is_renamed(spec, &own)?)?;
        if let (Some(target), Some(column)) = (&target, on_target) {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn spec(yaml: &str) -> MongoVistaSpec {
        serde_yaml_ng::from_str(yaml).unwrap()
    }

    const CLIENT: &str = r#"
name: client
columns:
  _id: { flags: [id] }
  name: {}
"#;

    fn resolver(target: &'static str) -> MongoSpecResolver {
        Arc::new(move |name: &str| (name == "client" || name == "order").then(|| spec(target)))
    }

    fn message(result: Result<()>) -> String {
        result.expect_err("renamed key must be refused").to_string()
    }

    #[test]
    fn nested_has_one_foreign_key_is_refused() {
        let order = spec(
            r#"
name: order
columns:
  _id: { flags: [id] }
  client_id: { mongo: { nested_path: refs.client } }
references:
  client: { table: client, kind: has_one, foreign_key: client_id }
"#,
        );
        let msg = message(check_spec(&order, None));
        assert!(msg.contains("its own field name"), "{msg}");
        assert!(msg.contains("client_id"), "{msg}");
    }

    const ORDER_BY_FIELD: &str = r#"
name: order
columns:
  _id: { flags: [id] }
  client_id: { mongo: { field: clientId } }
references:
  client: { table: client, kind: has_one, foreign_key: client_id }
"#;

    #[test]
    fn renamed_foreign_key_is_refused() {
        let msg = message(check_spec(&spec(ORDER_BY_FIELD), None));
        assert!(msg.contains("client_id"), "{msg}");
    }

    #[test]
    fn field_spelling_its_own_name_is_accepted() {
        let order = spec(&ORDER_BY_FIELD.replace("field: clientId", "field: client_id"));
        assert!(check_spec(&order, None).is_ok());
    }

    #[test]
    fn scripted_reference_is_exempt_with_rhai() {
        let order = spec(&ORDER_BY_FIELD.replace(
            "foreign_key: client_id }",
            r#"foreign_key: client_id, mongo: { rhai: 'table("client")' } }"#,
        ));
        let result = check_spec(&order, None);
        if cfg!(feature = "rhai") {
            assert!(result.is_ok(), "{result:?}");
        } else {
            message(result);
        }
    }

    #[test]
    fn nested_target_id_column_is_refused() {
        let order = spec(
            r#"
name: order
columns:
  _id: { flags: [id] }
  client_code: {}
references:
  client: { table: client, kind: has_one, foreign_key: client_code }
"#,
        );
        let nested_id = r#"
name: client
columns:
  code: { flags: [id], mongo: { nested_path: ids.code } }
"#;
        assert!(check_spec(&order, Some(&resolver(CLIENT))).is_ok());
        let msg = message(check_spec(&order, Some(&resolver(nested_id))));
        assert!(msg.contains("code"), "{msg}");
    }

    #[test]
    fn nested_has_many_foreign_key_on_target_is_refused() {
        let client = spec(
            r#"
name: client
columns:
  _id: { flags: [id] }
references:
  orders: { table: order, kind: has_many, foreign_key: client_id }
"#,
        );
        let nested_fk = r#"
name: order
columns:
  _id: { flags: [id] }
  client_id: { mongo: { nested_path: refs.client } }
"#;
        assert!(check_spec(&client, None).is_ok());
        let msg = message(check_spec(&client, Some(&resolver(nested_fk))));
        assert!(msg.contains("orders"), "{msg}");
    }
}
