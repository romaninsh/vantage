//! Declared column types against the schema's `DEFINE FIELD` types, as read
//! by [`SurrealDB::check_schema`]. A diagnostic only: the YAML type decides
//! what is written, and this names the columns where SurrealDB will refuse
//! it, so whoever edits the YAML can fix one side.

use std::collections::HashMap;

use vantage_core::{Context, VantageError};
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_source::SchemaMismatch;
use vantage_types::Entity;

use crate::surrealdb::SurrealDB;
use crate::surrealdb::declared::declared_name;
use crate::types::AnySurrealType;

impl SurrealDB {
    /// The stored columns of `table` whose declared type disagrees with the
    /// field's `DEFINE FIELD` type, or whose typed "no value" (`NONE`) the
    /// field refuses while it takes `NULL`. Empty before
    /// [`check_schema`](Self::check_schema) ran; fields the schema doesn't
    /// type (schemaless, `any`) are never reported.
    pub fn schema_mismatches<E: Entity<AnySurrealType>>(
        &self,
        table: &Table<SurrealDB, E>,
    ) -> Vec<SchemaMismatch> {
        let table_name = table.table_name();
        // Copied out: `has_one_target` below builds the target, and that
        // build (a roll-up asking `is_reference_link`) takes this lock again.
        let field_types: HashMap<String, String> = {
            let schema = self.references.lock().unwrap();
            let Some(field_types) = schema.field_types.as_ref() else {
                return Vec::new();
            };
            field_types
                .iter()
                .filter(|((table, _), _)| table == table_name)
                .map(|((_, field), kind)| (field.clone(), kind.clone()))
                .collect()
        };
        let id = table.id_field_name();
        table
            .columns()
            .iter()
            .filter(|(name, _)| **name != id && !table.is_calculated_column(name))
            .filter_map(|(name, column)| {
                let field = column.alias().unwrap_or(name);
                let stored = field_types.get(field)?;
                let declared = match table.has_one_target(name) {
                    Some(target) => Declared::Link(target),
                    None => Declared::Type(declared_name(column.get_type())?),
                };
                let hint = mismatch_hint(&declared, stored, table_name, field)?;
                Some(SchemaMismatch::new(
                    table_name,
                    name,
                    declared.to_string(),
                    stored,
                    hint,
                ))
            })
            .collect()
    }

    /// `tracing::warn!` each of [`schema_mismatches`](Self::schema_mismatches),
    /// once per column.
    pub(crate) fn warn_schema_mismatches<E: Entity<AnySurrealType>>(
        &self,
        table: &Table<SurrealDB, E>,
    ) {
        for mismatch in self.schema_mismatches(table) {
            let first = self
                .references
                .lock()
                .unwrap()
                .warned_mismatches
                .insert((mismatch.table.clone(), mismatch.column.clone()));
            if first {
                tracing::warn!(
                    table = mismatch.table,
                    column = mismatch.column,
                    "column `{}.{}` is declared `{}` but the field is `{}`: {}",
                    mismatch.table,
                    mismatch.column,
                    mismatch.declared,
                    mismatch.stored,
                    mismatch.hint
                );
            }
        }
    }
}

impl SurrealDB {
    /// `error`, re-raised naming the column and its mismatch when it is
    /// SurrealDB refusing to coerce a value for a field that
    /// [`schema_mismatches`](Self::schema_mismatches) lists; the database's
    /// error stays underneath. Any other error is returned as is.
    pub(crate) fn name_flagged_refusal<E: Entity<AnySurrealType>>(
        &self,
        table: &Table<SurrealDB, E>,
        error: VantageError,
    ) -> VantageError {
        let text = format!("{error:?}");
        let Some(field) = text
            .split("Couldn't coerce value for field `")
            .nth(1)
            .and_then(|rest| rest.split('`').next())
        else {
            return error;
        };
        let stored_as = |column: &str| {
            table
                .columns()
                .get(column)
                .and_then(|c| c.alias())
                .unwrap_or(column)
                .to_string()
        };
        let Some(mismatch) = self
            .schema_mismatches(table)
            .into_iter()
            .find(|m| stored_as(&m.column) == field)
        else {
            return error;
        };
        let named = vantage_core::error!(
            format!(
                "column `{}.{}` is declared `{}` but the field is `{}`: {}",
                mismatch.table, mismatch.column, mismatch.declared, mismatch.stored, mismatch.hint
            ),
            column = mismatch.column.as_str()
        )
        .mark_invalid_value();
        Err::<(), _>(error).context(named).unwrap_err()
    }
}

enum Declared {
    /// A has-one key column: links to the target table.
    Link(String),
    /// A YAML type name (`timestamp`).
    Type(&'static str),
}

impl std::fmt::Display for Declared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Declared::Link(target) => write!(f, "references: {target}"),
            Declared::Type(name) => f.write_str(name),
        }
    }
}

/// What to change when `declared` can't be written into a field of type
/// `stored`; `None` when they agree.
fn mismatch_hint(declared: &Declared, stored: &str, table: &str, field: &str) -> Option<String> {
    let kinds = alternatives(stored);
    if kinds.contains(&"any") {
        return None;
    }
    let define = format!("DEFINE FIELD {field} ON {table} TYPE");
    let fits = kinds.iter().any(|kind| fits(declared, kind));
    if !fits {
        return Some(match declared {
            Declared::Type("string") if kinds.contains(&"datetime") => {
                "declare `type: timestamp` (or `date`), so text is written as a datetime".into()
            }
            Declared::Type("string") if kinds.iter().any(|k| k.starts_with("record")) => {
                let target = kinds
                    .iter()
                    .find_map(|k| record_targets(k).first().copied())
                    .unwrap_or("<table>");
                format!(
                    "declare `references: {target}`, so `{target}:key` text is written as a record link"
                )
            }
            // A field left as text from before the YAML declared a link or
            // a datetime: convert the stored rows rather than the YAML.
            Declared::Link(target) if kinds.contains(&"string") => {
                convert_text(table, field, &format!("record<{target}>"), "type::record")
            }
            Declared::Type("timestamp" | "date") if kinds.contains(&"string") => {
                convert_text(table, field, "datetime", "type::datetime")
            }
            _ => format!("declare the `type:` the field holds, or `{define} …` to match the YAML"),
        });
    }
    let typed = !matches!(declared, Declared::Type("string"));
    let takes_null = kinds.contains(&"null");
    let takes_none = kinds.contains(&"none");
    (typed && takes_null && !takes_none).then(|| {
        format!(
            "a typed column writes NONE for no value, which the field refuses; \
             `{define} option<…>` takes it"
        )
    })
}

/// The hint for a text field the YAML now reads as `kind`: the SurrealQL that
/// retypes the field and converts the text its rows hold with `function`.
fn convert_text(table: &str, field: &str, kind: &str, function: &str) -> String {
    format!(
        "the field still holds text; convert it with `DEFINE FIELD OVERWRITE {field} ON {table} \
         TYPE {kind}; UPDATE {table} SET {field} = {function}({field}) \
         WHERE type::is_string({field});`, or declare `type: string`"
    )
}

/// Whether a value of `declared` fits one alternative of a field type.
fn fits(declared: &Declared, kind: &str) -> bool {
    let numeric = matches!(kind, "int" | "float" | "decimal" | "number");
    match declared {
        Declared::Link(target) => {
            kind == "record" || record_targets(kind).iter().any(|t| t == target)
        }
        Declared::Type(name) => match *name {
            "record" => kind == "record" || kind.starts_with("record<"),
            "timestamp" | "date" => kind == "datetime",
            "int" | "float" | "decimal" => numeric,
            "bool" => kind == "bool",
            "duration" => kind == "duration",
            "time" => kind == "string",
            // A column without `type:` is a string column too, so objects,
            // arrays and other shapes a string column may hold are left alone.
            "string" => {
                !numeric
                    && !matches!(kind, "datetime" | "bool" | "duration")
                    && !kind.starts_with("record")
            }
            _ => true,
        },
    }
}

/// The top-level alternatives of a field type: `option<datetime>` and
/// `none | datetime` both give `["none", "datetime"]`.
fn alternatives(stored: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (at, c) in stored.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                out.push(stored[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    out.push(stored[start..].trim());
    out.into_iter()
        .flat_map(|kind| {
            match kind
                .strip_prefix("option<")
                .and_then(|k| k.strip_suffix('>'))
            {
                Some(inner) => {
                    let mut kinds = vec!["none"];
                    kinds.extend(alternatives(inner));
                    kinds
                }
                None => vec![kind],
            }
        })
        .collect()
}

/// The tables a `record<a | b>` type links to; empty for any other type.
fn record_targets(kind: &str) -> Vec<&str> {
    kind.strip_prefix("record<")
        .and_then(|k| k.strip_suffix('>'))
        .map(|tables| tables.split('|').map(str::trim).collect())
        .unwrap_or_default()
}
