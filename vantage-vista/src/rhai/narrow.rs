//! Narrowing verbs on the `Table` handle: `table`, `where`, `sort`, `search`,
//! `limit`, `ref`. Each returns a new handle and never touches a backend.

use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult};

use super::convert::dynamic_to_cbor;
use super::handle::{Handle, Step};
use super::vocab::TargetResolver;
use crate::{FilterOp, sort::SortDirection};

type RhaiResult<T> = std::result::Result<T, Box<EvalAltResult>>;

/// Register the `Table` type and its narrowing verbs. `table(name)` exists only
/// when `resolver` is `Some`; the handle it returns carries that resolver.
pub(crate) fn register_narrowing(engine: &mut Engine, resolver: Option<TargetResolver>) {
    engine.register_type_with_name::<Handle>("Table");

    if let Some(resolver) = resolver {
        engine.register_fn("table", move |name: &str| {
            Handle::named(name).with_resolver(resolver.clone())
        });
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
            let op = FilterOp::parse(op).ok_or_else(|| {
                format!(
                    "unknown operator \"{op}\" in where (expected eq/ne/gt/gte/lt/lte/in/not_in/like)"
                )
            })?;
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

    engine.register_fn("ref", |h: &mut Handle, rel: &str| {
        h.push(Step::Ref(rel.into()))
    });
}

fn narrow_where(h: &Handle, col: &str, op: FilterOp, value: Dynamic) -> RhaiResult<Handle> {
    Ok(h.push(Step::Where {
        col: col.into(),
        op,
        value: dynamic_to_cbor(value)?,
    }))
}

fn parse_dir(dir: &str) -> RhaiResult<SortDirection> {
    match dir.to_ascii_lowercase().as_str() {
        "asc" | "ascending" => Ok(SortDirection::Ascending),
        "desc" | "descending" => Ok(SortDirection::Descending),
        other => Err(format!("invalid sort direction \"{other}\" (expected asc or desc)").into()),
    }
}
