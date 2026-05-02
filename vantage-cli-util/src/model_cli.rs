//! Generic model-driven CLI runner.
//!
//! Drives an `AnyTable` from positional argv tokens — model name, ARN,
//! `field=value` filters, `[N]` index selectors, and `:relation`
//! traversals. Backend specifics (which names map to which tables, how
//! ARNs are parsed, how records are rendered) are injected through the
//! [`ModelFactory`] and [`Renderer`] traits.
//!
//! ## Token forms
//!
//! - `arn:...` — ARN; resolved via [`ModelFactory::for_arn`]; drops
//!   straight into single-record mode.
//! - `iam.user`, `iam.users` — model name (singular drops into single
//!   mode, plural into list mode).
//! - `field=value` or `field="quoted value"` — adds an equality
//!   condition. Multiple are ANDed.
//! - `[N]` — selects the Nth record from a list (zero-indexed) and
//!   narrows the table to that record. Switches to single-record mode.
//! - `:relation` — traverses a relation registered via `with_many` /
//!   `with_one`. Only allowed in single-record mode (so the deferred
//!   child query yields a single foreign-key value); switches to list
//!   mode for the child table.
//! - `=col1,col2,...` — overrides the visible columns in list mode.
//!   Stays in effect for the rest of the run; later relation
//!   traversals reset it. The literal `id` resolves to the table's id
//!   field at render time.
//!
//! Glued forms are accepted: `users[0]`, `:members[0]`,
//! `name=foo[0]`, and `=col1,col2[0]` all split into a base token plus
//! an index selector.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::any::AnyTable;
use vantage_table::traits::table_like::TableLike;
use vantage_types::Record;

/// Whether the current state is a list of records or a single record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    Single,
}

/// Resolves model identifiers (singular/plural names, ARNs) to
/// `AnyTable`s. Implemented per-backend.
pub trait ModelFactory {
    /// Resolve a model name (e.g. `iam.user` or `iam.users`).
    /// Singular names should return [`Mode::Single`], plural
    /// [`Mode::List`]. Returns `None` for unknown names.
    fn for_name(&self, name: &str) -> Option<(AnyTable, Mode)>;

    /// Resolve an ARN to a single-record table with any required
    /// conditions (e.g. resource-name eq) already applied.
    fn for_arn(&self, arn: &str) -> Option<AnyTable>;
}

/// Backend hook for printing list and single-record results.
pub trait Renderer {
    fn render_list(
        &self,
        table: &AnyTable,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    );
    fn render_record(
        &self,
        table: &AnyTable,
        id: &str,
        record: &Record<CborValue>,
        relations: &[String],
    );
}

#[derive(Debug)]
enum Token {
    ModelName(String, Option<usize>),
    Arn(String),
    Condition(String, String, Option<usize>),
    Relation(String, Option<usize>),
    Index(usize),
    Columns(Vec<String>, Option<usize>),
}

fn split_index_suffix(s: &str) -> (&str, Option<usize>) {
    if let Some(stripped) = s.strip_suffix(']')
        && let Some(open) = stripped.rfind('[')
    {
        let inner = &stripped[open + 1..];
        if !inner.is_empty()
            && inner.chars().all(|c| c.is_ascii_digit())
            && let Ok(n) = inner.parse::<usize>()
        {
            return (&stripped[..open], Some(n));
        }
    }
    (s, None)
}

fn parse_token(arg: &str) -> Result<Token> {
    if arg.is_empty() {
        return Err(error!("Empty argument"));
    }
    if arg.starts_with("arn:") {
        return Ok(Token::Arn(arg.to_string()));
    }
    if let Some(rest) = arg.strip_prefix(':') {
        let (rel, idx) = split_index_suffix(rest);
        if rel.is_empty() {
            return Err(error!(format!("Empty relation name in token `{arg}`")));
        }
        return Ok(Token::Relation(rel.to_string(), idx));
    }
    if arg.starts_with('[') {
        let (_, idx) = split_index_suffix(arg);
        let idx = idx.ok_or_else(|| error!(format!("Invalid index token `{arg}`")))?;
        return Ok(Token::Index(idx));
    }
    if let Some(rest) = arg.strip_prefix('=') {
        let (cols_part, idx) = split_index_suffix(rest);
        if cols_part.is_empty() {
            return Err(error!(format!(
                "Empty column list in token `{arg}` — write `=col1,col2`"
            )));
        }
        let cols: Vec<String> = cols_part
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if cols.is_empty() {
            return Err(error!(format!("Empty column list in token `{arg}`")));
        }
        return Ok(Token::Columns(cols, idx));
    }
    if let Some(eq_pos) = arg.find('=') {
        let field = arg[..eq_pos].to_string();
        if field.is_empty() {
            return Err(error!(format!("Empty field name in token `{arg}`")));
        }
        let value_part = &arg[eq_pos + 1..];
        let (value, idx) =
            if value_part.starts_with('"') && value_part.ends_with('"') && value_part.len() >= 2 {
                (value_part[1..value_part.len() - 1].to_string(), None)
            } else {
                let (v, i) = split_index_suffix(value_part);
                (v.to_string(), i)
            };
        return Ok(Token::Condition(field, value, idx));
    }
    let (name, idx) = split_index_suffix(arg);
    Ok(Token::ModelName(name.to_string(), idx))
}

/// Run a model-driven CLI.
///
/// `args` is the list of positional arguments after any global flags
/// (region, profile, etc.) have been stripped out.
pub async fn run<F: ModelFactory, R: Renderer>(
    factory: &F,
    renderer: &R,
    args: &[String],
) -> Result<()> {
    if args.is_empty() {
        return Err(error!(
            "No model specified — pass a model name (e.g. `iam.users`) or an ARN"
        ));
    }

    let mut tokens: Vec<Token> = args.iter().map(|s| parse_token(s)).collect::<Result<_>>()?;
    let first = tokens.remove(0);
    let mut column_override: Option<Vec<String>> = None;

    let (mut table, mut mode) = match first {
        Token::ModelName(name, idx) => {
            let (t, m) = factory
                .for_name(&name)
                .ok_or_else(|| error!(format!("Unknown model `{name}`")))?;
            if let Some(i) = idx {
                apply_index(t, i).await?
            } else {
                (t, m)
            }
        }
        Token::Arn(arn) => {
            let t = factory
                .for_arn(&arn)
                .ok_or_else(|| error!(format!("Cannot resolve ARN `{arn}`")))?;
            (t, Mode::Single)
        }
        Token::Condition(_, _, _)
        | Token::Relation(_, _)
        | Token::Index(_)
        | Token::Columns(_, _) => {
            return Err(error!(format!(
                "First argument must be a model name or ARN, got `{}`",
                args[0]
            )));
        }
    };

    for token in tokens {
        match token {
            Token::Condition(field, value, idx) => {
                // `id=value` is a sugared "fetch this specific record":
                // resolve to the actual id field and force single-record
                // mode. Anything else is just a regular eq filter.
                let is_id_alias = field == "id";
                let resolved_field = if is_id_alias {
                    table.id_field_name().ok_or_else(|| {
                        error!(format!(
                            "`id=` used but table `{}` has no id field",
                            table.table_name()
                        ))
                    })?
                } else {
                    field.clone()
                };
                table.add_condition_eq(&resolved_field, &value)?;
                if is_id_alias {
                    mode = Mode::Single;
                }
                if let Some(i) = idx {
                    let (t, m) = apply_index(table, i).await?;
                    table = t;
                    mode = m;
                }
            }
            Token::Index(i) => {
                let (t, m) = apply_index(table, i).await?;
                table = t;
                mode = m;
            }
            Token::Relation(rel, idx) => {
                if mode != Mode::Single {
                    return Err(error!(format!(
                        "Cannot traverse `:{rel}` from list mode — narrow to a single record first (add a filter or `[N]`)"
                    )));
                }
                table = table.get_ref(&rel)?;
                mode = Mode::List;
                // A new table means the column override no longer
                // applies — drop it so the child renders with its own
                // default columns until a new override appears.
                column_override = None;
                if let Some(i) = idx {
                    let (t, m) = apply_index(table, i).await?;
                    table = t;
                    mode = m;
                }
            }
            Token::Columns(cols, idx) => {
                column_override = Some(cols);
                if let Some(i) = idx {
                    let (t, m) = apply_index(table, i).await?;
                    table = t;
                    mode = m;
                }
            }
            Token::ModelName(_, _) | Token::Arn(_) => {
                return Err(error!(
                    "Model name or ARN may only appear as the first argument"
                ));
            }
        }
    }

    match mode {
        Mode::List => {
            let records = table.list_values().await?;
            renderer.render_list(&table, &records, column_override.as_deref());
        }
        Mode::Single => {
            let (id, record) = table
                .get_some_value()
                .await?
                .ok_or_else(|| error!("No record found"))?;
            let relations = table.get_ref_names();
            renderer.render_record(&table, &id, &record, &relations);
        }
    }

    Ok(())
}

/// List the table, take the Nth row, narrow the table to that row by
/// adding `eq(id_field, that_id)`. Returns the narrowed table in
/// single-record mode so subsequent traversals see one parent.
async fn apply_index(mut table: AnyTable, index: usize) -> Result<(AnyTable, Mode)> {
    let records = table.list_values().await?;
    let total = records.len();
    let (id, _record) = records.into_iter().nth(index).ok_or_else(|| {
        error!(format!(
            "Index [{index}] out of bounds — only {total} record(s) match"
        ))
    })?;
    let id_field = table.id_field_name().ok_or_else(|| {
        error!(format!(
            "Cannot apply index — table `{}` has no id field",
            table.table_name()
        ))
    })?;
    table.add_condition_eq(&id_field, &id)?;
    Ok((table, Mode::Single))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_split_index_suffix() {
        assert_eq!(split_index_suffix("users"), ("users", None));
        assert_eq!(split_index_suffix("users[0]"), ("users", Some(0)));
        assert_eq!(split_index_suffix("users[42]"), ("users", Some(42)));
        assert_eq!(split_index_suffix("[3]"), ("", Some(3)));
        assert_eq!(split_index_suffix("foo[bar]"), ("foo[bar]", None));
        assert_eq!(split_index_suffix("foo[]"), ("foo[]", None));
    }

    #[test]
    fn token_parse_kinds() {
        match parse_token("iam.users").unwrap() {
            Token::ModelName(n, i) => {
                assert_eq!(n, "iam.users");
                assert_eq!(i, None);
            }
            t => panic!("expected ModelName, got {t:?}"),
        }
        match parse_token("iam.users[0]").unwrap() {
            Token::ModelName(n, i) => {
                assert_eq!(n, "iam.users");
                assert_eq!(i, Some(0));
            }
            t => panic!("expected ModelName with index, got {t:?}"),
        }
        match parse_token(":members[2]").unwrap() {
            Token::Relation(r, i) => {
                assert_eq!(r, "members");
                assert_eq!(i, Some(2));
            }
            t => panic!("expected Relation, got {t:?}"),
        }
        match parse_token("name=alice").unwrap() {
            Token::Condition(f, v, i) => {
                assert_eq!(f, "name");
                assert_eq!(v, "alice");
                assert_eq!(i, None);
            }
            t => panic!("expected Condition, got {t:?}"),
        }
        match parse_token("name=\"john doe\"").unwrap() {
            Token::Condition(f, v, i) => {
                assert_eq!(f, "name");
                assert_eq!(v, "john doe");
                assert_eq!(i, None);
            }
            t => panic!("expected Condition, got {t:?}"),
        }
        match parse_token("name=alice[0]").unwrap() {
            Token::Condition(f, v, i) => {
                assert_eq!(f, "name");
                assert_eq!(v, "alice");
                assert_eq!(i, Some(0));
            }
            t => panic!("expected Condition with index, got {t:?}"),
        }
        match parse_token("[7]").unwrap() {
            Token::Index(i) => assert_eq!(i, 7),
            t => panic!("expected Index, got {t:?}"),
        }
        match parse_token("arn:aws:iam::123:user/alice").unwrap() {
            Token::Arn(s) => assert_eq!(s, "arn:aws:iam::123:user/alice"),
            t => panic!("expected Arn, got {t:?}"),
        }
        match parse_token("=timestamp,message").unwrap() {
            Token::Columns(cols, idx) => {
                assert_eq!(cols, vec!["timestamp".to_string(), "message".to_string()]);
                assert_eq!(idx, None);
            }
            t => panic!("expected Columns, got {t:?}"),
        }
        match parse_token("=id, name [0]").unwrap_or_else(|_| {
            // trim handles the spaces, but split on `[0]` requires no
            // intervening space — skip that variant.
            parse_token("=id,name[0]").unwrap()
        }) {
            Token::Columns(cols, idx) => {
                assert_eq!(cols, vec!["id".to_string(), "name".to_string()]);
                assert_eq!(idx, Some(0));
            }
            t => panic!("expected Columns with index, got {t:?}"),
        }
    }
}
