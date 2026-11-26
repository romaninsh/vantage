//! Narrowing verbs on the `Table` handle: `table`, `where`, `sort`, `search`,
//! `limit`, `offset`, `ref`. Each returns a new handle and never touches a backend.

use std::sync::Arc;

use vantage_rhai::ModuleDir;
use vantage_rhai::rhai::{Dynamic, Engine, FnPtr, NativeCallContext};

use super::convert::{condition_arg, register_null};
use super::handle::{Handle, Step};
use super::read::RhaiResult;
use crate::{FilterOp, VistaResolver, sort::SortDirection};

/// Register the `Table` type and its narrowing verbs. `table(name)` exists only
/// when `resolver` is `Some`; the handle it returns carries that resolver.
pub(crate) fn register_narrowing(engine: &mut Engine, resolver: Option<Arc<dyn VistaResolver>>) {
    engine.register_type_with_name::<Handle>("Table");
    register_null(engine);

    if let Some(resolver) = resolver {
        engine.register_fn(
            "table",
            move |ctx: NativeCallContext, name: &str| -> RhaiResult<Handle> {
                if ModuleDir::is_helper_call(&ctx) {
                    return Err(format!(
                        "table() isn't available in lib/ helpers; pass table(\"{name}\") in as an argument"
                    )
                    .into());
                }
                Ok(Handle::named(name).with_resolver(resolver.clone()))
            },
        );
    }

    engine.register_fn(
        "where",
        |h: &mut Handle, col: &str, value: Dynamic| -> RhaiResult<Handle> {
            narrow_where(h, col, FilterOp::Eq, value)
        },
    );
    engine.register_fn(
        "where",
        |h: &mut Handle, col: &str, op: &str, value: Dynamic| -> RhaiResult<Handle> {
            let op = FilterOp::parse(op).ok_or_else(|| unknown_operator(op))?;
            narrow_where(h, col, op, value)
        },
    );

    engine.register_fn("sort", |h: &mut Handle, col: &str| {
        h.push(Step::Sort {
            col: col.into(),
            dir: SortDirection::Ascending,
        })
    });
    engine.register_fn(
        "sort",
        |h: &mut Handle, col: &str, dir: &str| -> RhaiResult<Handle> {
            Ok(h.push(Step::Sort {
                col: col.into(),
                dir: parse_dir(dir)?,
            }))
        },
    );

    engine.register_fn("search", |h: &mut Handle, text: &str| {
        h.push(Step::Search(text.into()))
    });

    engine.register_fn("limit", |h: &mut Handle, n: i64| -> RhaiResult<Handle> {
        if n <= 0 {
            return Err("limit must be greater than 0".into());
        }
        Ok(h.push(Step::Limit(n as usize)))
    });

    engine.register_fn("offset", |h: &mut Handle, n: i64| -> RhaiResult<Handle> {
        if n < 0 {
            return Err("offset can't be negative".into());
        }
        Ok(h.push(Step::Offset(n as usize)))
    });

    engine.register_fn("ref", |h: &mut Handle, rel: &str| {
        h.push(Step::Ref(rel.into()))
    });

    // `where_expr(|t| t.ref("tags").where("tag_id", 5).exists())`: the
    // closure gets the backend's table and returns a boolean query
    // expression, which narrows the handle as a native condition.
    engine.register_fn(
        "where_expr",
        |ctx: NativeCallContext, h: &mut Handle, f: FnPtr| -> RhaiResult<Handle> {
            h.with_built_condition("where_expr", |vista| {
                let needs = || {
                    vantage_core::error!(
                        format!(
                            "`where_expr` needs a backend that takes native conditions; `{}` doesn't",
                            vista.name()
                        ),
                        table = vista.name()
                    )
                };
                if !vista.capabilities().has(crate::Capability::NativeConditions) {
                    return Err(needs());
                }
                let table = vista.script_table().ok_or_else(needs)?;
                let expr: Dynamic = f
                    .call_within_context(&ctx, (table,))
                    .map_err(|e| vantage_core::error!(format!("where_expr: {e}")))?;
                vista.script_condition(expr)
            })
            .map_err(|e| e.to_string().into())
        },
    );
}

fn narrow_where(h: &Handle, col: &str, op: FilterOp, value: Dynamic) -> RhaiResult<Handle> {
    Ok(h.push(Step::Where {
        col: col.into(),
        op,
        value: condition_arg(col, op, value)?,
    }))
}

/// The error of an operator `where` doesn't know.
pub(crate) fn unknown_operator(op: &str) -> String {
    format!(
        "unknown operator \"{op}\" in where (expected eq/ne/gt/gte/lt/lte/in/not_in/like/is/is_not)"
    )
}

fn parse_dir(dir: &str) -> RhaiResult<SortDirection> {
    match dir.to_ascii_lowercase().as_str() {
        "asc" | "ascending" => Ok(SortDirection::Ascending),
        "desc" | "descending" => Ok(SortDirection::Descending),
        other => Err(format!("invalid sort direction \"{other}\" (expected asc or desc)").into()),
    }
}
