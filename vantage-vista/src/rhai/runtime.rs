//! One-shot runners for agent tools: [`run_script`] evaluates a data script
//! and returns its value as JSON; [`preview_script`] renders the query a
//! script describes without reading its rows.
//!
//! Rhai is synchronous and Vista reads are async. [`run_script`] evaluates
//! inside [`tokio::task::spawn_blocking`], so each terminal verb can drive its
//! future through the [bridge](super::block_on) on the runtime.

use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};

use super::handle::Handle;
use super::vocab::{DataVocab, TargetResolver, Writes};

/// Lower bound applied to a requested row limit.
pub const MIN_LIMIT: usize = 1;
/// Most rows any single `list()` returns from [`run_script`].
pub const MAX_LIMIT: usize = 50;
/// Limit used when a caller does not specify one.
pub const DEFAULT_LIMIT: usize = 5;

/// Compile and run a one-shot script, bypassing the host cache.
fn run_once(host: &Host, script: &str) -> Result<Dynamic, String> {
    host.compile_uncached(&Block::from(script))
        .and_then(|s| s.eval(&Env::new()))
        .map_err(|e| e.to_string())
}

/// Evaluate a data script with read and write terminals and return its final
/// value as JSON.
///
/// `resolver` backs `table(name)`. `limit` is clamped to
/// `[MIN_LIMIT, MAX_LIMIT]` and caps every `list()`. `writes` decides whether
/// the write verbs act or throw. Compile, runtime and backend errors all come
/// back as the script error's text.
pub async fn run_script(
    script: String,
    resolver: TargetResolver,
    limit: usize,
    writes: Writes,
) -> Result<serde_json::Value, String> {
    let limit = limit.clamp(MIN_LIMIT, MAX_LIMIT);
    tokio::task::spawn_blocking(move || {
        let host = Host::builder(Limits::background())
            .vocab(DataVocab::read_write_with(
                Some(resolver),
                Some(limit),
                writes,
            ))
            .build();
        run_once(&host, &script).map(|v| vantage_rhai::to_json(&v))
    })
    .await
    .map_err(|e| format!("data script task failed to run: {e}"))?
}

/// Render the query a script describes, without reading it.
///
/// The host has no terminal verbs, so the script can't spell a read of the
/// set it describes; its final expression must be the handle itself:
///
/// ```rhai
/// table("orders").where("status", "paid").limit(20)
/// ```
///
/// Returns the driver-shaped JSON from
/// [`TableShell::preview_query`](crate::TableShell::preview_query). A `ref`
/// step still reads the rows it traverses from, through the resolver and the
/// [bridge](super::block_on), so an async caller runs this under
/// [`tokio::task::spawn_blocking`].
pub fn preview_script(
    script: String,
    resolver: TargetResolver,
) -> Result<serde_json::Value, String> {
    let host = Host::builder(Limits::background())
        .vocab(DataVocab::describe(Some(resolver.clone())))
        .build();
    let handle = run_once(&host, &script)?
        .try_cast::<Handle>()
        .ok_or_else(|| {
            "preview script must end on the query itself, e.g. \
         `table(\"orders\").where(\"status\", \"paid\")`; this host has no \
         terminal verbs (`list`, `count`, `first`)"
                .to_string()
        })?;
    let vista = handle.resolve(Some(&resolver)).map_err(|e| e.to_string())?;
    Ok(vista.preview_query())
}
