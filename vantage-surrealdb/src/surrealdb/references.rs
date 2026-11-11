//! The database schema as `INFO FOR TABLE` declares it: which record links
//! are declared `REFERENCE`, and each field's `TYPE`. Only `REFERENCE` links
//! are visible to `<~` lookups, so a roll-up renders the fast `<~` form over
//! them and the correlated `$parent` form over any other link. Field types
//! only feed diagnostics (see `schema`): no write depends on them.

use std::collections::{HashMap, HashSet};

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
    /// `(table, field)` → the field's `TYPE` as printed; `None` until checked.
    pub(crate) field_types: Option<HashMap<(String, String), String>>,
    /// Links already warned about, so a rebuilt table doesn't warn again.
    warned: HashSet<(String, String)>,
    /// `(table, column)` schema mismatches already warned about.
    pub(crate) warned_mismatches: HashSet<(String, String)>,
}

impl SurrealDB {
    /// Read the database schema (`INFO FOR DB`, then `INFO FOR TABLE` on each
    /// table): which record links are declared `REFERENCE`, and each field's
    /// `TYPE`. Call it after connecting, and again after changing the schema.
    ///
    /// Roll-up columns built before the check, or over a link without
    /// `REFERENCE`, use the slower correlated `$parent` form. Field types
    /// feed [`schema_mismatches`](Self::schema_mismatches) only; what a
    /// write stores never depends on them.
    pub async fn check_schema(&self) -> Result<()> {
        let db: JsonValue = self.execute(&surreal_expr!("INFO FOR DB")).await?.into();
        let tables = db
            .get("tables")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| error!("INFO FOR DB returned no tables"))?;

        let mut links = HashSet::new();
        let mut field_types = HashMap::new();
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
                let Some(definition) = definition.as_str() else {
                    continue;
                };
                let key = (table.clone(), field.clone());
                if declares_reference(definition) {
                    links.insert(key.clone());
                }
                if let Some(kind) = declared_type(definition) {
                    field_types.insert(key, kind);
                }
            }
        }

        let mut schema = self.references.lock().unwrap();
        schema.links = Some(links);
        schema.field_types = Some(field_types);
        Ok(())
    }

    /// The former name of [`check_schema`](Self::check_schema).
    #[deprecated(since = "2.3.0", note = "renamed to `check_schema`")]
    pub async fn check_references(&self) -> Result<()> {
        self.check_schema().await
    }

    /// Whether `table.field` is a `REFERENCE` link; `None` before
    /// [`check_schema`](Self::check_schema) ran.
    pub(crate) fn is_reference_link(&self, table: &str, field: &str) -> Option<bool> {
        let refs = self.references.lock().unwrap();
        let links = refs.links.as_ref()?;
        Some(links.contains(&(table.to_string(), field.to_string())))
    }

    /// Warn, once per link, that a roll-up over `child.field` reads through
    /// the correlated `$parent` form. `linked` is the table the field's
    /// records point at, when known.
    pub(crate) fn warn_slow_rollup(
        &self,
        child: &str,
        field: &str,
        linked: Option<&str>,
        checked: bool,
    ) {
        let first = self
            .references
            .lock()
            .unwrap()
            .warned
            .insert((child.to_string(), field.to_string()));
        if !first {
            return;
        }
        let record = linked.map_or("record".to_string(), |table| format!("record<{table}>"));
        let define = format!("DEFINE FIELD {field} ON {child} TYPE {record} REFERENCE;");
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
                 the schema wasn't checked (SurrealDB::check_schema); if the field is \
                 declared REFERENCE (`{define}`), check it to use `<~`"
            );
        }
    }
}

/// A field definition without its comment, which may hold any word.
fn without_comment(definition: &str) -> &str {
    definition
        .split_once(" COMMENT ")
        .map_or(definition, |(head, _)| head)
}

/// A field definition from `INFO FOR TABLE` declares `REFERENCE`, outside its
/// comment.
fn declares_reference(definition: &str) -> bool {
    without_comment(definition)
        .split_whitespace()
        .any(|word| word == "REFERENCE")
}

/// The `TYPE` of a field definition as printed (`none | datetime`,
/// `record<client>`), up to the next clause keyword; `None` for a field
/// without one. Types print in lowercase and clause keywords in uppercase.
pub(crate) fn declared_type(definition: &str) -> Option<String> {
    let (_, rest) = without_comment(definition).split_once(" TYPE ")?;
    let words: Vec<&str> = rest
        .split_whitespace()
        .take_while(|word| !is_keyword(word))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

fn is_keyword(word: &str) -> bool {
    word.len() > 1 && word.chars().all(|c| c.is_ascii_uppercase())
}
