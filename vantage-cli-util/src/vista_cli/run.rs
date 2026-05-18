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

/// In-flight runner state: vista, mode, accumulated column override, and
/// pending aggregate. Kept in one struct so token application is a single
/// `&mut self` call rather than a wad of locals threaded through every branch.
struct RunState {
    vista: Vista,
    mode: Mode,
    column_override: Option<Vec<String>>,
    aggregate: Option<(AggregateOp, Option<String>)>,
}

impl RunState {
    /// Consume the first token to obtain the initial Vista + mode.
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
                    .ok_or_else(|| error!(format!("Unknown model `{name}`")))?;
                let m = apply_selector_opt(&mut v, m, sel, renderer).await?;
                (v, m)
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
                    "First argument must be a model name or locator, got `{first_arg}`"
                )));
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
                self.vista.add_search(query)?;
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
            return Err(error!(format!(
                "Cannot traverse `:{rel}` from list mode — narrow to a single record first (add a filter or `[N]`)"
            )));
        }
        let child_kind = self
            .vista
            .list_references()
            .into_iter()
            .find(|(name, _)| name == &rel)
            .map(|(_, k)| k);
        let (_id, parent_row) = self.vista.get_some_value().await?.ok_or_else(|| {
            error!(format!(
                "Cannot traverse `:{rel}` — narrowed vista has no matching record"
            ))
        })?;
        self.vista = self.vista.get_ref(&rel, &parent_row)?;
        self.mode = match child_kind {
            Some(ReferenceKind::HasOne) => Mode::Single,
            _ => Mode::List,
        };
        // A new Vista deserves its own default columns.
        self.column_override = None;
        self.mode = apply_selector_opt(&mut self.vista, self.mode, sel, renderer).await?;
        Ok(())
    }
}

/// Aggregates short-circuit normal list/single rendering with a single
/// scalar. The underlying `Vista::get_sum` / `get_count` / etc. APIs
/// haven't landed yet, so this still notes a stub and renders `Null`.
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
            let relations: Vec<String> = vista
                .get_references()
                .iter()
                .map(|s| s.to_string())
                .collect();
            renderer.render_record(vista, &id, &record, &relations);
        }
    }
    Ok(())
}

/// Apply a `[…]` selector to `vista` in place. Returns the new mode
/// (single when a `[N]` index slice narrowed to one record, otherwise
/// the incoming mode). Sort routes through `Vista::add_order`; slice's
/// `Index` variant uses the real narrow-to-single path. Slice's `Range`
/// variant is stubbed — Vista's pagination surface is page-based, so an
/// arbitrary `[start:end]` offset doesn't map cleanly yet.
async fn apply_selector<R: Renderer>(
    vista: &mut Vista,
    mode: Mode,
    sel: Selector,
    renderer: &R,
) -> Result<Mode> {
    if let Some((field, dir)) = &sel.sort {
        vista.add_order(field, sort_direction(*dir))?;
    }
    match sel.slice {
        None => Ok(mode),
        Some(Slice::Index(n)) => apply_index(vista, n).await,
        Some(Slice::Range { start, end }) => {
            // TODO: vista.set_pagination(start, end) once Vista grows an
            // offset-style range primitive — today's surface is page-based.
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

/// Apply an operator condition. Returns `Some(Mode::Single)` when the
/// condition uses the `id=` alias (which forces single-record mode),
/// `None` otherwise. Only `Op::Eq` is wired; all other ops note a stub
/// and return without mutating the vista.
///
/// For `Op::Eq`, `value_raw` carries the original user text when the
/// parser took the auto-detect path. Run-time coercion against the
/// target column's declared type then wins, so `name=true` on a string
/// column produces `Text("true")` rather than `Bool(true)`. When
/// `value_raw` is `None` the user explicitly forced a type via
/// `#literal` and `value` is honoured as-is.
fn apply_condition<R: Renderer>(
    vista: &mut Vista,
    field: &str,
    op: Op,
    value: Option<CborValue>,
    value_raw: Option<String>,
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
            let coerced = match value_raw {
                Some(raw) => super::value::coerce_for_column(vista, &resolved_field, &raw)?,
                None => v,
            };
            vista.add_condition_eq(&resolved_field, coerced)?;
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
/// adding `eq(id_field, that_id)`. Returns `Mode::Single` so subsequent
/// traversals see one parent.
async fn apply_index(vista: &mut Vista, index: usize) -> Result<Mode> {
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
    let coerced = super::value::coerce_for_column(vista, &id_field, &id)?;
    vista.add_condition_eq(&id_field, coerced)?;
    Ok(Mode::Single)
}
