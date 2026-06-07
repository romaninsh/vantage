//! The Rhai engine: registers `run`, `parse_json`, `parse_jsonl`, seeds
//! the query variables into scope, evaluates the script, and converts the
//! returned array-of-maps into JSON rows.
//!
//! This whole function is synchronous and is invoked from inside
//! `tokio::task::spawn_blocking` — the engine and the subprocess call
//! never cross a thread boundary, so the async runtime stays unblocked.

use std::path::Path;
use std::sync::Arc;

use indexmap::IndexMap;
use rhai::{Dynamic, Engine, EvalAltResult, Map, Position, Scope};
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};

use crate::condition::CmdCondition;
use crate::exec::run_command;

/// Everything the script needs about the current read, seeded as scope
/// variables before evaluation.
pub(crate) struct QueryContext {
    pub conditions: Vec<CmdCondition>,
    pub columns: Vec<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub id_column: Option<String>,
    /// The target id for a per-row detail fetch, seeded as the `id` scope
    /// variable. `None` for list reads.
    pub id: Option<String>,
    /// The existing (list-pass) record for a detail fetch, seeded as the `row`
    /// scope variable so the detail script can read cheap columns it carries.
    /// An empty map for list reads and id-only detail reads.
    pub row: ciborium::value::Value,
}

fn runtime_err(msg: impl Into<String>) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(msg.into()),
        Position::NONE,
    ))
}

fn dynamic_to_arg(d: Dynamic) -> String {
    if d.is_string() {
        d.into_string().unwrap_or_default()
    } else {
        d.to_string()
    }
}

/// A rhai script with its `Engine` and parsed `AST` built once and reused
/// for every evaluation. The locked `command`/`env` are baked into the
/// engine's `run` binding at construction, so a [`CompiledScript`] is
/// specific to one table's script + command + env.
///
/// Requires rhai's `sync` feature so `Engine`/`AST` are `Send + Sync` and
/// the compiled script can be cached on the (clone-shared) [`Cmd`] and
/// evaluated across `spawn_blocking` threads.
pub(crate) struct CompiledScript {
    engine: Engine,
    ast: rhai::AST,
}

impl CompiledScript {
    /// Build the engine (registering `run`/`parse_json`/`parse_jsonl`) and
    /// compile `script` to an `AST`. Both are done once; [`eval`](Self::eval)
    /// reuses them.
    pub(crate) fn compile(
        command: String,
        env: IndexMap<String, String>,
        pass_path: bool,
        base_dir: Option<Arc<Path>>,
        script: &str,
    ) -> Result<Self> {
        let mut engine = Engine::new();
        engine.set_max_expr_depths(256, 256);

        // parse_json(string) -> Dynamic
        engine.register_fn(
            "parse_json",
            |s: &str| -> std::result::Result<Dynamic, Box<EvalAltResult>> {
                let v: JsonValue =
                    serde_json::from_str(s).map_err(|e| runtime_err(format!("parse_json: {e}")))?;
                rhai::serde::to_dynamic(v)
            },
        );

        // parse_jsonl(string) -> array of Dynamic, one per non-empty line
        engine.register_fn(
            "parse_jsonl",
            |s: &str| -> std::result::Result<Dynamic, Box<EvalAltResult>> {
                let mut out = rhai::Array::new();
                for line in s.lines().filter(|l| !l.trim().is_empty()) {
                    let v: JsonValue = serde_json::from_str(line)
                        .map_err(|e| runtime_err(format!("parse_jsonl: {e}")))?;
                    out.push(rhai::serde::to_dynamic(v)?);
                }
                Ok(out.into())
            },
        );

        // run(args) -> #{ stdout, stderr, exit_code }. Command + env are
        // captured here, NOT passed by the script — that's the security lock.
        engine.register_fn(
            "run",
            move |args: rhai::Array| -> std::result::Result<Map, Box<EvalAltResult>> {
                let argv: Vec<String> = args.into_iter().map(dynamic_to_arg).collect();
                let out = run_command(&command, &argv, &env, pass_path, base_dir.as_deref())
                    .map_err(|e| runtime_err(e.to_string()))?;
                let mut map = Map::new();
                map.insert("stdout".into(), out.stdout.into());
                map.insert("stderr".into(), out.stderr.into());
                map.insert("exit_code".into(), (out.exit_code as i64).into());
                Ok(map)
            },
        );

        let ast = engine
            .compile(script)
            .map_err(|e| error!("command rhai script failed to compile", detail = e.to_string()))?;

        Ok(Self { engine, ast })
    }

    /// Evaluate the compiled script with the `ctx` variables seeded into a
    /// fresh scope, returning the rows the script produced as JSON objects.
    pub(crate) fn eval(&self, ctx: QueryContext) -> Result<Vec<JsonValue>> {
        let mut scope = Scope::new();
        scope.push_dynamic("conditions", conditions_dynamic(&ctx.conditions)?);
        scope.push_dynamic("columns", to_dynamic(&ctx.columns)?);
        scope.push_dynamic("limit", opt_int(ctx.limit));
        scope.push_dynamic("offset", opt_int(ctx.offset));
        scope.push_dynamic("id_column", opt_string(ctx.id_column));
        scope.push_dynamic("id", opt_string(ctx.id));
        scope.push_dynamic("row", to_dynamic(&ctx.row)?);

        let result: Dynamic = self
            .engine
            .eval_ast_with_scope(&mut scope, &self.ast)
            .map_err(|e| error!("command rhai script failed", detail = e.to_string()))?;

        if !result.is_array() {
            return Err(error!(
                "command rhai script must return an array of rows",
                got = result.type_name().to_string()
            ));
        }
        let arr = result
            .into_array()
            .map_err(|t| error!("expected array result", got = t.to_string()))?;

        arr.into_iter()
            .map(|row| {
                serde_json::to_value(&row)
                    .map_err(|e| error!("failed to convert row to JSON", detail = e.to_string()))
            })
            .collect()
    }
}

fn opt_int(v: Option<i64>) -> Dynamic {
    v.map(Dynamic::from).unwrap_or(Dynamic::UNIT)
}

fn opt_string(v: Option<String>) -> Dynamic {
    v.map(Dynamic::from).unwrap_or(Dynamic::UNIT)
}

fn to_dynamic<T: serde::Serialize>(v: &T) -> Result<Dynamic> {
    rhai::serde::to_dynamic(v)
        .map_err(|e| error!("failed to seed rhai scope", detail = e.to_string()))
}

fn conditions_dynamic(conditions: &[CmdCondition]) -> Result<Dynamic> {
    let arr: Vec<JsonValue> = conditions
        .iter()
        .map(|c| {
            serde_json::json!({
                "field": c.field(),
                "op": c.op(),
                "value": c.json_value(),
            })
        })
        .collect();
    to_dynamic(&arr)
}
