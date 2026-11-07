//! Which record links are declared `REFERENCE`. Only those are visible to
//! `<~` lookups, so a roll-up renders the fast `<~` form over them and the
//! correlated `$parent` form over any other link.

use std::collections::HashSet;

use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_expressions::ExprDataSource;

use crate::identifier::Identifier;
use crate::surreal_expr;
use crate::surrealdb::SurrealDB;

#[derive(Default)]
pub(crate) struct ReferenceLinks {
    /// `(table, field)` of every `REFERENCE` link; `None` until checked.
    links: Option<HashSet<(String, String)>>,
    /// Links already warned about, so a rebuilt table doesn't warn again.
    warned: HashSet<(String, String)>,
}

impl SurrealDB {
    /// Read the database schema (`INFO FOR DB`, then `INFO FOR TABLE` on each
    /// table) and remember which record links are declared `REFERENCE`.
    /// Call it after connecting, and again after changing the schema.
    ///
    /// Roll-up columns built before the check, or over a link without
    /// `REFERENCE`, use the slower correlated `$parent` form.
    pub async fn check_references(&self) -> Result<()> {
        let db: JsonValue = self.execute(&surreal_expr!("INFO FOR DB")).await?.into();
        let tables = db
            .get("tables")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| error!("INFO FOR DB returned no tables"))?;

        let mut links = HashSet::new();
        for table in tables.keys() {
            let info: JsonValue = self
                .execute(&surreal_expr!(
                    "INFO FOR TABLE {}",
                    (Identifier::new(table.as_str()))
                ))
                .await?
                .into();
            let Some(fields) = info.get("fields").and_then(JsonValue::as_object) else {
                continue;
            };
            for (field, definition) in fields {
                if definition.as_str().is_some_and(declares_reference) {
                    links.insert((table.clone(), field.clone()));
                }
            }
        }

        self.references.lock().unwrap().links = Some(links);
        Ok(())
    }

    /// Whether `table.field` is a `REFERENCE` link; `None` before
    /// [`check_references`](Self::check_references) ran.
    pub(crate) fn is_reference_link(&self, table: &str, field: &str) -> Option<bool> {
        let refs = self.references.lock().unwrap();
        let links = refs.links.as_ref()?;
        Some(links.contains(&(table.to_string(), field.to_string())))
    }

    /// Warn, once per link, that a roll-up over `child.field` reads through
    /// the correlated `$parent` form.
    pub(crate) fn warn_slow_rollup(&self, child: &str, field: &str, parent: &str, checked: bool) {
        let first = self
            .references
            .lock()
            .unwrap()
            .warned
            .insert((child.to_string(), field.to_string()));
        if !first {
            return;
        }
        let define = format!("DEFINE FIELD {field} ON {child} TYPE record<{parent}> REFERENCE;");
        if checked {
            tracing::warn!(
                table = child,
                field = field,
                "roll-up over `{child}.{field}` uses a correlated `$parent` subquery, because \
                 the field isn't declared REFERENCE; `{define}` makes it use `<~`"
            );
        } else {
            tracing::warn!(
                table = child,
                field = field,
                "roll-up over `{child}.{field}` uses a correlated `$parent` subquery, because \
                 the schema wasn't checked (SurrealDB::check_references); if the field is \
                 declared REFERENCE (`{define}`), check it to use `<~`"
            );
        }
    }
}

/// A field definition from `INFO FOR TABLE` declares `REFERENCE`, outside its
/// comment.
fn declares_reference(definition: &str) -> bool {
    let definition = definition
        .split_once(" COMMENT ")
        .map_or(definition, |(head, _)| head);
    definition
        .split_whitespace()
        .any(|word| word == "REFERENCE")
}
