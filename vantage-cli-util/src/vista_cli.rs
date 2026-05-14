//! Generic model-driven CLI runner, Vista edition.
//!
//! Mirrors [`crate::model_cli`] but drives a [`Vista`] instead of an
//! `AnyTable`. The token shapes and flow are identical (`model_name`,
//! `field=value`, `[N]`, `:relation`, `=col1,col2`) — only the
//! underlying type changes.
//!
//! See `model_cli` for the full token-shape reference; this module
//! repeats only the behaviour notes that differ for Vista.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::Vista;

/// Whether the current state is a list of records or a single record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    Single,
}

/// Resolves model identifiers (singular/plural names, ARNs) to
/// `Vista`s. Implemented per-backend.
pub trait ModelFactory {
    /// Resolve a model name (e.g. `users` or `user`).
    /// Singular names should return [`Mode::Single`], plural
    /// [`Mode::List`]. Returns `None` for unknown names.
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)>;

    /// Resolve an ARN to a single-record `Vista` with any required
    /// conditions already applied. Backends without an ARN syntax
    /// can leave the default `None`.
    fn for_arn(&self, _arn: &str) -> Option<Vista> {
        None
    }
}

/// Backend hook for printing list and single-record results.
pub trait Renderer {
    fn render_list(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        column_override: Option<&[String]>,
    );
    fn render_record(
        &self,
        vista: &Vista,
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

/// Run a Vista-backed model-driven CLI.
///
/// `args` is the list of positional arguments after any global flags
/// have been stripped out.
pub async fn run<F: ModelFactory, R: Renderer>(
    factory: &F,
    renderer: &R,
    args: &[String],
) -> Result<()> {
    if args.is_empty() {
        return Err(error!(
            "No model specified — pass a model name (e.g. `users`) or an ARN"
        ));
    }

    let mut tokens: Vec<Token> = args.iter().map(|s| parse_token(s)).collect::<Result<_>>()?;
    let first = tokens.remove(0);
    let mut column_override: Option<Vec<String>> = None;

    let (mut vista, mut mode) = match first {
        Token::ModelName(name, idx) => {
            let (v, m) = factory
                .for_name(&name)
                .ok_or_else(|| error!(format!("Unknown model `{name}`")))?;
            if let Some(i) = idx {
                apply_index(v, i).await?
            } else {
                (v, m)
            }
        }
        Token::Arn(arn) => {
            let v = factory
                .for_arn(&arn)
                .ok_or_else(|| error!(format!("Cannot resolve ARN `{arn}`")))?;
            (v, Mode::Single)
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
                    vista.get_id_column().map(str::to_string).ok_or_else(|| {
                        error!(format!(
                            "`id=` used but vista `{}` has no id column",
                            vista.name()
                        ))
                    })?
                } else {
                    field.clone()
                };
                vista.add_condition_eq(&resolved_field, string_to_cbor(&value))?;
                if is_id_alias {
                    mode = Mode::Single;
                }
                if let Some(i) = idx {
                    let (v, m) = apply_index(vista, i).await?;
                    vista = v;
                    mode = m;
                }
            }
            Token::Index(i) => {
                let (v, m) = apply_index(vista, i).await?;
                vista = v;
                mode = m;
            }
            Token::Relation(rel, idx) => {
                if mode != Mode::Single {
                    return Err(error!(format!(
                        "Cannot traverse `:{rel}` from list mode — narrow to a single record first (add a filter or `[N]`)"
                    )));
                }
                vista = vista.get_ref(&rel)?;
                mode = Mode::List;
                // A new vista means the column override no longer
                // applies — drop it so the child renders with its own
                // default columns until a new override appears.
                column_override = None;
                if let Some(i) = idx {
                    let (v, m) = apply_index(vista, i).await?;
                    vista = v;
                    mode = m;
                }
            }
            Token::Columns(cols, idx) => {
                column_override = Some(cols);
                if let Some(i) = idx {
                    let (v, m) = apply_index(vista, i).await?;
                    vista = v;
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
            let records = vista.list_values().await?;
            renderer.render_list(&vista, &records, column_override.as_deref());
        }
        Mode::Single => {
            let (id, record) = vista
                .get_some_value()
                .await?
                .ok_or_else(|| error!("No record found"))?;
            let relations: Vec<String> =
                vista.get_references().iter().map(|s| s.to_string()).collect();
            renderer.render_record(&vista, &id, &record, &relations);
        }
    }

    Ok(())
}

/// CLI tokens carry filter values as plain strings. Coerce them into
/// the cheapest matching CBOR scalar: integer if it parses, else
/// float, else booleans, else text. Drivers translate further at
/// their own boundary.
fn string_to_cbor(value: &str) -> CborValue {
    if let Ok(i) = value.parse::<i64>() {
        CborValue::Integer(i.into())
    } else if let Ok(f) = value.parse::<f64>() {
        CborValue::Float(f)
    } else if value == "true" {
        CborValue::Bool(true)
    } else if value == "false" {
        CborValue::Bool(false)
    } else {
        CborValue::Text(value.to_string())
    }
}

/// List the vista, take the Nth row, narrow the vista to that row by
/// adding `eq(id_field, that_id)`. Returns the narrowed vista in
/// single-record mode so subsequent traversals see one parent.
async fn apply_index(mut vista: Vista, index: usize) -> Result<(Vista, Mode)> {
    let records = vista.list_values().await?;
    let total = records.len();
    let (id, _record) = records.into_iter().nth(index).ok_or_else(|| {
        error!(format!(
            "Index [{index}] out of bounds — only {total} record(s) match"
        ))
    })?;
    let id_field = vista.get_id_column().map(str::to_string).ok_or_else(|| {
        error!(format!(
            "Cannot apply index — vista `{}` has no id column",
            vista.name()
        ))
    })?;
    vista.add_condition_eq(&id_field, string_to_cbor(&id))?;
    Ok((vista, Mode::Single))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_split_index_suffix() {
        assert_eq!(split_index_suffix("users"), ("users", None));
        assert_eq!(split_index_suffix("users[0]"), ("users", Some(0)));
        assert_eq!(split_index_suffix("[3]"), ("", Some(3)));
        assert_eq!(split_index_suffix("foo[bar]"), ("foo[bar]", None));
    }

    #[test]
    fn token_parse_relation() {
        match parse_token(":albums[2]").unwrap() {
            Token::Relation(r, i) => {
                assert_eq!(r, "albums");
                assert_eq!(i, Some(2));
            }
            t => panic!("expected Relation, got {t:?}"),
        }
    }
}
