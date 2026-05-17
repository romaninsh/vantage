//! Runner: walks the parsed token stream and drives the Vista.
//!
//! Wired through to real Vista calls: `Op::Eq` conditions, `[N]`
//! narrow-to-single, `:relation` traversal, column overrides, sort
//! (`[+col]` / `[-col]` → `Vista::add_order`), and search (`?keyword`
//! → `Vista::add_search`). Still stubbed (the `Renderer::note_stub`
//! hook reports the call name): operator vocabulary beyond `eq`,
//! range slicing (`[N:M]`), and aggregates. Drivers without the
//! matching `can_*` capability return `Unsupported` from the
//! corresponding Vista method.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{ReferenceKind, SortDirection, Vista};

use super::factory::{ModelFactory, Renderer};
use super::parse::parse_token;
use super::token::{AggregateOp, Direction, Mode, Op, Selector, Slice, Token};

/// Run a Vista-backed model-driven CLI.
///
/// `args` is the list of positional arguments after any global flags
/// (region, profile, `--format=…`, …) have been stripped by the caller.
pub async fn run<F: ModelFactory, R: Renderer>(
    factory: &F,
    renderer: &R,
    args: &[String],
) -> Result<()> {
    if args.is_empty() {
        return Err(error!(
            "No model specified — pass a model name (e.g. `users`) or a locator"
        ));
    }

    let mut tokens: Vec<Token> = args.iter().map(|s| parse_token(s)).collect::<Result<_>>()?;
    let first = tokens.remove(0);
    let mut column_override: Option<Vec<String>> = None;
    let mut aggregate: Option<(AggregateOp, Option<String>)> = None;

    let (mut vista, mut mode) = match first {
        Token::ModelName(name, sel) => {
            let (v, m) = factory
                .for_name(&name)
                .ok_or_else(|| error!(format!("Unknown model `{name}`")))?;
            apply_selector_opt(v, m, sel, renderer).await?
        }
        Token::Locator(s) => {
            let v = factory
                .for_locator(&s)
                .ok_or_else(|| error!(format!("Cannot resolve locator `{s}`")))?;
            (v, Mode::Single)
        }
        Token::OpCondition { .. }
        | Token::Relation(_, _)
        | Token::Bracket(_)
        | Token::Columns(_, _)
        | Token::Search(_)
        | Token::Aggregate { .. } => {
            return Err(error!(format!(
                "First argument must be a model name or locator, got `{}`",
                args[0]
            )));
        }
    };

    for token in tokens {
        if aggregate.is_some() {
            return Err(error!(
                "Aggregate token (`@op:field`) must be the last argument"
            ));
        }
        match token {
            Token::ModelName(_, _) | Token::Locator(_) => {
                return Err(error!(
                    "Model name or locator may only appear as the first argument"
                ));
            }
            Token::OpCondition {
                field,
                op,
                value,
                selector,
            } => {
                if let Some(new_mode) = apply_condition(&mut vista, &field, op, value, renderer)? {
                    mode = new_mode;
                }
                if let Some(sel) = selector {
                    let (v, m) = apply_selector(vista, mode, sel, renderer).await?;
                    vista = v;
                    mode = m;
                }
            }
            Token::Relation(rel, sel) => {
                if mode != Mode::Single {
                    return Err(error!(format!(
                        "Cannot traverse `:{rel}` from list mode — narrow to a single record first (add a filter or `[N]`)"
                    )));
                }
                let child_kind = vista
                    .list_references()
                    .into_iter()
                    .find(|(name, _)| name == &rel)
                    .map(|(_, k)| k);
                let (_id, parent_row) = vista.get_some_value().await?.ok_or_else(|| {
                    error!(format!(
                        "Cannot traverse `:{rel}` — narrowed vista has no matching record"
                    ))
                })?;
                vista = vista.get_ref(&rel, &parent_row)?;
                mode = match child_kind {
                    Some(ReferenceKind::HasOne) => Mode::Single,
                    _ => Mode::List,
                };
                column_override = None;
                if let Some(sel) = sel {
                    let (v, m) = apply_selector(vista, mode, sel, renderer).await?;
                    vista = v;
                    mode = m;
                }
            }
            Token::Bracket(sel) => {
                let (v, m) = apply_selector(vista, mode, sel, renderer).await?;
                vista = v;
                mode = m;
            }
            Token::Columns(cols, sel) => {
                column_override = Some(cols);
                if let Some(sel) = sel {
                    let (v, m) = apply_selector(vista, mode, sel, renderer).await?;
                    vista = v;
                    mode = m;
                }
            }
            Token::Search(query) => {
                vista.add_search(query)?;
            }
            Token::Aggregate { op, field } => {
                aggregate = Some((op, field));
            }
        }
    }

    if let Some((op, field)) = aggregate {
        // TODO: vista.get_sum / get_max / get_min / get_count(field) once
        // stage 5b lands. Stub returns null so the format renderers still
        // produce something coherent.
        renderer.note_stub(&format!(
            "{}({})",
            op.name(),
            field.as_deref().unwrap_or("*")
        ));
        renderer.render_scalar(&vista, op, field.as_deref(), &CborValue::Null);
        return Ok(());
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
            let relations: Vec<String> = vista
                .get_references()
                .iter()
                .map(|s| s.to_string())
                .collect();
            renderer.render_record(&vista, &id, &record, &relations);
        }
    }

    Ok(())
}

/// Apply a `[…]` selector: sort then slice. Sort routes through
/// `Vista::add_order`; slice's `Index` variant uses the real
/// narrow-to-single path. Slice's `Range` variant is stubbed — Vista's
/// pagination surface is page-based (`set_page_size` + `fetch_page`),
/// so an arbitrary `[start:end]` offset doesn't map cleanly yet.
async fn apply_selector<R: Renderer>(
    vista: Vista,
    mode: Mode,
    sel: Selector,
    renderer: &R,
) -> Result<(Vista, Mode)> {
    let mut vista = vista;
    let mut mode = mode;

    if let Some((field, dir)) = &sel.sort {
        vista.add_order(field, sort_direction(*dir))?;
    }
    if let Some(slice) = sel.slice {
        match slice {
            Slice::Index(n) => {
                let (v, m) = apply_index(vista, n).await?;
                vista = v;
                mode = m;
            }
            Slice::Range { start, end } => {
                // TODO: vista.set_pagination(start, end) once Vista grows an
                // offset-style range primitive — today's surface is page-based.
                renderer.note_stub(&format!("set_pagination({start}, {end:?})"));
            }
        }
    }
    Ok((vista, mode))
}

fn sort_direction(dir: Direction) -> SortDirection {
    match dir {
        Direction::Asc => SortDirection::Ascending,
        Direction::Desc => SortDirection::Descending,
    }
}

async fn apply_selector_opt<R: Renderer>(
    vista: Vista,
    mode: Mode,
    opt_sel: Option<Selector>,
    renderer: &R,
) -> Result<(Vista, Mode)> {
    match opt_sel {
        Some(sel) => apply_selector(vista, mode, sel, renderer).await,
        None => Ok((vista, mode)),
    }
}

/// Apply an operator condition. Returns `Some(Mode::Single)` when the
/// condition uses the `id=` alias (which forces single-record mode),
/// `None` otherwise. Only `Op::Eq` is wired; all other ops note a stub
/// and return without mutating the vista.
fn apply_condition<R: Renderer>(
    vista: &mut Vista,
    field: &str,
    op: Op,
    value: Option<CborValue>,
    renderer: &R,
) -> Result<Option<Mode>> {
    if op.is_nullary() {
        // TODO: vista.add_condition(field, op, None) once stage 5 lands.
        renderer.note_stub(&format!("add_condition({field:?}, {})", op.name()));
        return Ok(None);
    }
    let v = value.ok_or_else(|| error!("internal: value-bearing operator missing value"))?;
    match op {
        Op::Eq => {
            let is_id_alias = field == "id";
            let resolved_field = if is_id_alias {
                vista.get_id_column().map(str::to_string).ok_or_else(|| {
                    error!(format!(
                        "`id=` used but vista `{}` has no id column",
                        vista.name()
                    ))
                })?
            } else {
                field.to_string()
            };
            vista.add_condition_eq(&resolved_field, v)?;
            Ok(if is_id_alias {
                Some(Mode::Single)
            } else {
                None
            })
        }
        _ => {
            // TODO: vista.add_condition(field, op, v) once stage 5 lands.
            renderer.note_stub(&format!("add_condition({field:?}, {}, {v:?})", op.name()));
            Ok(None)
        }
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
    vista.add_condition_eq(&id_field, super::value::auto_detect(&id))?;
    Ok((vista, Mode::Single))
}
