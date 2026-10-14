//! Runner: walks the parsed token stream and drives the Vista.
//!
//! Wired to Vista: `Op::Eq` conditions, `[N]` narrowing, `:relation`
//! traversal, column overrides, sort (`Vista::set_order`) and search
//! (`Vista::set_search`). Reported through `Renderer::note_stub` instead:
//! operators beyond `eq`, range slicing (`[N:M]`) and aggregates.
//! Drivers lacking the matching [`Capability`](vantage_vista::Capability)
//! return `Unsupported` from the Vista method.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{FilterOp, ReferenceKind, SortDirection, Vista};

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
    let mut state = RunState::resolve_first(factory, renderer, first, &args[0]).await?;

    for token in tokens {
        if state.aggregate.is_some() {
            return Err(error!(
                "Aggregate token (`@op:field`) must be the last argument"
            ));
        }
        state.apply_token(renderer, token).await?;
    }

    if let Some((op, field)) = state.aggregate {
        return render_aggregate(renderer, &state.vista, op, field);
    }
    render_final(
        renderer,
        &state.vista,
        state.mode,
        state.column_override.as_deref(),
    )
    .await
}

struct RunState {
    vista: Vista,
    mode: Mode,
    column_override: Option<Vec<String>>,
    aggregate: Option<(AggregateOp, Option<String>)>,
}

impl RunState {
    async fn resolve_first<F: ModelFactory, R: Renderer>(
        factory: &F,
        renderer: &R,
        first: Token,
        first_arg: &str,
    ) -> Result<Self> {
        let (vista, mode) = match first {
            Token::ModelName(name, sel) => {
                let (mut v, m) = factory
                    .for_name(&name)
                    .ok_or_else(|| error!("Unknown model", model = name))?;
                let m = apply_selector_opt(&mut v, m, sel, renderer).await?;
                (v, m)
            }
            Token::Locator(s) => {
                let v = factory
                    .for_locator(&s)
                    .ok_or_else(|| error!("Cannot resolve locator", locator = s))?;
                (v, Mode::Single)
            }
            Token::OpCondition { .. }
            | Token::Relation(_, _)
            | Token::Bracket(_)
            | Token::Columns(_, _)
            | Token::Search(_)
            | Token::Aggregate { .. } => {
                return Err(error!(
                    "First argument must be a model name or locator",
                    got = first_arg
                ));
            }
        };
        Ok(Self {
            vista,
            mode,
            column_override: None,
            aggregate: None,
        })
    }

    async fn apply_token<R: Renderer>(&mut self, renderer: &R, token: Token) -> Result<()> {
        match token {
            Token::ModelName(_, _) | Token::Locator(_) => Err(error!(
                "Model name or locator may only appear as the first argument"
            )),
            Token::OpCondition {
                field,
                op,
                value,
                value_raw,
                selector,
            } => {
                if let Some(new_mode) =
                    apply_condition(&mut self.vista, &field, op, value, value_raw, renderer)?
                {
                    self.mode = new_mode;
                }
                self.mode =
                    apply_selector_opt(&mut self.vista, self.mode, selector, renderer).await?;
                Ok(())
            }
            Token::Relation(rel, sel) => self.apply_relation(renderer, rel, sel).await,
            Token::Bracket(sel) => {
                self.mode = apply_selector(&mut self.vista, self.mode, sel, renderer).await?;
                Ok(())
            }
            Token::Columns(cols, sel) => {
                self.column_override = Some(cols);
                self.mode = apply_selector_opt(&mut self.vista, self.mode, sel, renderer).await?;
                Ok(())
            }
            Token::Search(query) => {
                self.vista.set_search(Some(&query))?;
                Ok(())
            }
            Token::Aggregate { op, field } => {
                self.aggregate = Some((op, field));
                Ok(())
            }
        }
    }

    async fn apply_relation<R: Renderer>(
        &mut self,
        renderer: &R,
        rel: String,
        sel: Option<Selector>,
    ) -> Result<()> {
        if self.mode != Mode::Single {
            return Err(error!(
                "Cannot traverse a relation from list mode — narrow to a single record first (add a filter or `[N]`)",
                relation = rel
            ));
        }
        let child_kind = self.vista.reference(&rel).map(|r| r.kind);
        let (_id, parent_row) = self.vista.get_some_value().await?.ok_or_else(|| {
            error!(
                "Cannot traverse relation — narrowed vista has no matching record",
                relation = rel
            )
        })?;
        self.vista = self.vista.get_ref(&rel, Some(&parent_row))?;
        self.mode = match child_kind {
            Some(ReferenceKind::HasOne | ReferenceKind::ContainsOne) => Mode::Single,
            _ => Mode::List,
        };
        self.column_override = None;
        self.mode = apply_selector_opt(&mut self.vista, self.mode, sel, renderer).await?;
        Ok(())
    }
}

// Stub: Vista has no aggregate API yet, so this renders `Null`.
fn render_aggregate<R: Renderer>(
    renderer: &R,
    vista: &Vista,
    op: AggregateOp,
    field: Option<String>,
) -> Result<()> {
    renderer.note_stub(&format!(
        "{}({})",
        op.name(),
        field.as_deref().unwrap_or("*")
    ));
    renderer.render_scalar(vista, op, field.as_deref(), &CborValue::Null);
    Ok(())
}

async fn render_final<R: Renderer>(
    renderer: &R,
    vista: &Vista,
    mode: Mode,
    column_override: Option<&[String]>,
) -> Result<()> {
    match mode {
        Mode::List => {
            let records = vista.list_values().await?;
            renderer.render_list(vista, &records, column_override);
        }
        Mode::Single => {
            let (id, record) = vista
                .get_some_value()
                .await?
                .ok_or_else(|| error!("No record found"))?;
            let relations: Vec<String> = vista.references().keys().cloned().collect();
            renderer.render_record(vista, &id, &record, &relations);
        }
    }
    Ok(())
}

/// Returns `Mode::Single` when an `[N]` index narrowed to one record,
/// otherwise the incoming mode. Sort is applied before slicing.
async fn apply_selector<R: Renderer>(
    vista: &mut Vista,
    mode: Mode,
    sel: Selector,
    renderer: &R,
) -> Result<Mode> {
    if let Some((field, dir)) = &sel.sort {
        vista.set_order(Some((field, sort_direction(*dir))))?;
    }
    match sel.slice {
        None => Ok(mode),
        Some(Slice::Index(n)) => apply_index(vista, n).await,
        Some(Slice::Range { start, end }) => {
            // TODO: Vista's pagination is page-based; needs an offset range primitive.
            renderer.note_stub(&format!("set_pagination({start}, {end:?})"));
            Ok(mode)
        }
    }
}

fn sort_direction(dir: Direction) -> SortDirection {
    match dir {
        Direction::Asc => SortDirection::Ascending,
        Direction::Desc => SortDirection::Descending,
    }
}

async fn apply_selector_opt<R: Renderer>(
    vista: &mut Vista,
    mode: Mode,
    opt_sel: Option<Selector>,
    renderer: &R,
) -> Result<Mode> {
    match opt_sel {
        Some(sel) => apply_selector(vista, mode, sel, renderer).await,
        None => Ok(mode),
    }
}

/// Returns `Some(Mode::Single)` for the `id=` alias, `None` otherwise.
/// Only `Op::Eq` reaches the vista; other ops note a stub.
///
/// With `value_raw` set, the column's declared type decides the value
/// (`name=true` on a string column is `Text("true")`); without it the
/// user forced a type via `#literal` and `value` is used as-is.
fn apply_condition<R: Renderer>(
    vista: &mut Vista,
    field: &str,
    op: Op,
    value: Option<CborValue>,
    value_raw: Option<String>,
    renderer: &R,
) -> Result<Option<Mode>> {
    if op.is_nullary() {
        // TODO: wire nullary ops to Vista conditions.
        renderer.note_stub(&format!("add_condition({field:?}, {})", op.name()));
        return Ok(None);
    }
    let v = value.ok_or_else(|| error!("internal: value-bearing operator missing value"))?;
    match op {
        Op::Eq => {
            let is_id_alias = field == "id";
            let resolved_field = if is_id_alias {
                vista.id_column().map(str::to_string).ok_or_else(|| {
                    error!(
                        "`id=` used but vista has no id column",
                        vista = vista.name()
                    )
                })?
            } else {
                field.to_string()
            };
            let coerced = match value_raw {
                Some(raw) => super::value::coerce_for_column(vista, &resolved_field, &raw)?,
                None => v,
            };
            vista.add_condition(&resolved_field, FilterOp::Eq, coerced)?;
            Ok(if is_id_alias {
                Some(Mode::Single)
            } else {
                None
            })
        }
        _ => {
            // TODO: wire the remaining operators to Vista conditions.
            renderer.note_stub(&format!("add_condition({field:?}, {}, {v:?})", op.name()));
            Ok(None)
        }
    }
}

/// Narrow the vista to its Nth row with an `eq` condition on the id column.
async fn apply_index(vista: &mut Vista, index: usize) -> Result<Mode> {
    let records = vista.list_values().await?;
    let total = records.len();
    let (id, _record) = records.into_iter().nth(index).ok_or_else(|| {
        error!(
            "Index out of bounds",
            index = index,
            matching_records = total
        )
    })?;
    let id_field = vista.id_column().map(str::to_string).ok_or_else(|| {
        error!(
            "Cannot apply index — vista has no id column",
            vista = vista.name()
        )
    })?;
    let coerced = super::value::coerce_for_column(vista, &id_field, &id)?;
    vista.add_condition(&id_field, FilterOp::Eq, coerced)?;
    Ok(Mode::Single)
}
