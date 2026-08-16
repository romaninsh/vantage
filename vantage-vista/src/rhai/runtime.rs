//! The `run_script` runner: evaluate a data-fetch script and return its final
//! value as JSON.
//!
//! Rhai is synchronous; the Vista fetch methods are async. [`run_script`] runs
//! the whole evaluation inside [`tokio::task::spawn_blocking`], which gives a
//! thread with a runtime *context* but no async *frame* — so each terminal verb
//! can legally `Handle::current().block_on(…)` its fetch (see
//! [`super::fetch`]). Calling `block_on` directly from an async task (e.g. an
//! MCP handler future) would instead panic, which is exactly why the
//! `spawn_blocking` hop exists.
//!
//! ## Traversal idiom (load, then descend only if loaded)
//!
//! ```rhai
//! let o = table("orders").get_some();          // load one record
//! if o != () {                                 // only traverse if it loaded
//!     table("orders").get_ref("client", o).get_some()
//! }
//! ```

use rhai::{Dynamic, Engine};

use super::conventional::{RhaiVista, TargetResolver, register_conventional_onto};
use super::convert::dynamic_to_json;
use super::fetch::register_fetch_verbs;

/// Lower bound applied to a requested row limit.
pub const MIN_LIMIT: usize = 1;
/// Hard ceiling on rows any single `list()` returns — this is a debug surface,
/// not a bulk reader.
pub const MAX_LIMIT: usize = 50;
/// Limit used when a caller does not specify one.
pub const DEFAULT_LIMIT: usize = 5;

/// Evaluate a data-fetch script and return its final value as JSON.
///
/// `resolver` backs the `table(name)` constructor — pass a *direct* resolver
/// (fresh master Vista) or a *cache* resolver (a live Dio's cache-backed Vista);
/// this is indifferent to which. `limit` is clamped to `[MIN_LIMIT, MAX_LIMIT]`
/// and caps every `list()` in the script.
///
/// Compile errors, runtime errors, and resolver/fetch failures all surface as
/// `Err(String)` (the Rhai error message), so a caller can hand the agent a
/// precise complaint instead of a generic failure.
pub async fn run_script(
    script: String,
    resolver: TargetResolver,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let limit = limit.clamp(MIN_LIMIT, MAX_LIMIT);

    tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let mut engine = Engine::new();
        // The conventional builder vocabulary (table/condition/order/get_ref…)…
        register_conventional_onto(&mut engine, resolver);
        // …plus the terminal fetch/introspection verbs.
        register_fetch_verbs(&mut engine, limit);

        let result: Dynamic = engine.eval::<Dynamic>(&script).map_err(|e| e.to_string())?;
        Ok(dynamic_to_json(&result))
    })
    .await
    .map_err(|e| format!("data-fetch script task failed to run: {e}"))?
}

/// Build a query with the conventional verbs and render what it *would* send,
/// without sending anything.
///
/// The engine this runs on carries [`register_conventional_onto`] and nothing
/// else — the terminal fetch verbs are deliberately absent, so there is no path
/// from a preview script to a backend read. That is the point: preview is safe
/// to expose in places where fetching is not, and the safety is structural
/// rather than a promise about how the verb is used.
///
/// Consequently the script takes no terminal verb at all. Its final expression
/// must *be* the built query:
///
/// ```rhai
/// table("orders").add_condition_eq("status", "paid").set_page_size(20)
/// ```
///
/// A reference can be previewed against a literal parent row, since `get_ref`
/// reads the join value straight out of the map it is handed:
///
/// ```rhai
/// table("orders").get_ref("client", #{ id: "42" })
/// ```
///
/// Synchronous throughout: the resolver builds vistas without fetching, and
/// [`TableShell::preview_query`](crate::TableShell::preview_query) is
/// contractually I/O-free. Returns the driver-shaped JSON that method produced.
pub fn preview_script(
    script: String,
    resolver: TargetResolver,
) -> Result<serde_json::Value, String> {
    let mut engine = Engine::new();
    register_conventional_onto(&mut engine, resolver);

    let result: Dynamic = engine.eval::<Dynamic>(&script).map_err(|e| e.to_string())?;
    let handle: RhaiVista = result.try_cast::<RhaiVista>().ok_or_else(|| {
        "preview script must end on the query itself, e.g. \
         `table(\"orders\").add_condition_eq(\"status\", \"paid\")` — this engine \
         has no terminal verbs (`list`, `count`, `get_some`), because it never \
         fetches"
            .to_string()
    })?;

    let vista = handle
        .0
        .lock()
        .map_err(|_| "preview script: result mutex poisoned".to_string())?
        .take()
        .ok_or_else(|| "preview script: vista already consumed".to_string())?;

    Ok(vista.preview_query())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::MockShell;
    use crate::vista::Vista;
    use crate::{Column, VistaMetadata};
    use ciborium::Value as CborValue;
    use std::sync::Arc;
    use vantage_types::Record;

    fn cbor_text(s: &str) -> CborValue {
        CborValue::Text(s.into())
    }

    fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    /// Three users, rebuilt fresh on each `table("users")` call.
    fn users_vista() -> Vista {
        let source = MockShell::new()
            .with_record(
                "1",
                record(&[("id", cbor_text("1")), ("name", cbor_text("Alice"))]),
            )
            .with_record(
                "2",
                record(&[("id", cbor_text("2")), ("name", cbor_text("Bob"))]),
            )
            .with_record(
                "3",
                record(&[("id", cbor_text("3")), ("name", cbor_text("Carol"))]),
            );
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(
                Column::new("name", "String")
                    .with_flag("title")
                    .with_flag("orderable"),
            )
            .with_id_column("id");
        Vista::new("users", Box::new(source.with_metadata(metadata)))
    }

    fn resolver() -> TargetResolver {
        Arc::new(|name: &str| {
            if name == "users" {
                Ok(users_vista())
            } else {
                Err(vantage_core::error!("unknown table", table = name))
            }
        })
    }

    /// `users` with a `posts` has-many hanging off it, joined on `author`.
    fn users_with_posts() -> Vista {
        let posts = MockShell::new();
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_id_column("id")
            .with_reference(crate::Reference::new(
                "posts",
                "posts",
                crate::ReferenceKind::HasMany,
                "author",
            ));
        let source = MockShell::new()
            .with_metadata(metadata)
            .with_ref_target("posts", posts);
        Vista::new("users", Box::new(source))
    }

    fn ref_resolver() -> TargetResolver {
        Arc::new(|name: &str| {
            if name == "users" {
                Ok(users_with_posts())
            } else {
                Err(vantage_core::error!("unknown table", table = name))
            }
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_caps_rows_at_limit() {
        let json = run_script(r#"table("users").list()"#.into(), resolver(), 2)
            .await
            .unwrap();
        let rows = json.as_array().expect("array");
        assert_eq!(rows.len(), 2, "limit caps the returned rows");
        assert_eq!(rows[0]["name"], serde_json::json!("Alice"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_clamps_to_max() {
        // Requesting 9999 must clamp to MAX_LIMIT, not return everything-unbounded.
        let json = run_script(r#"table("users").list()"#.into(), resolver(), 9999)
            .await
            .unwrap();
        // Only 3 rows exist, but the point is the clamp didn't error/over-cap.
        assert_eq!(json.as_array().unwrap().len(), 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_some_returns_a_map() {
        let json = run_script(r#"table("users").get_some()"#.into(), resolver(), 5)
            .await
            .unwrap();
        assert!(json.get("id").is_some(), "get_some yields a record object");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn condition_then_get_some_narrows() {
        let json = run_script(
            r#"table("users").add_condition_eq("id", "3").get_some()"#.into(),
            resolver(),
            5,
        )
        .await
        .unwrap();
        assert_eq!(json["name"], serde_json::json!("Carol"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn capabilities_is_a_flag_map() {
        let json = run_script(r#"table("users").capabilities()"#.into(), resolver(), 5)
            .await
            .unwrap();
        assert!(json.get("can_fetch_window").is_some());
        assert!(json["can_count"].is_boolean());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn columns_lists_schema() {
        let json = run_script(r#"table("users").columns()"#.into(), resolver(), 5)
            .await
            .unwrap();
        let cols = json.as_array().unwrap();
        assert!(cols.iter().any(|c| c["name"] == serde_json::json!("name")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_table_is_an_error() {
        let err = run_script(r#"table("ghosts").list()"#.into(), resolver(), 5)
            .await
            .unwrap_err();
        assert!(err.contains("unknown table"), "got: {err}");
    }

    #[test]
    fn preview_renders_the_built_query() {
        let json = preview_script(
            r#"table("users").add_condition_eq("id", "3").add_order("name", "desc")"#.into(),
            resolver(),
        )
        .unwrap();
        assert_eq!(json["driver"], serde_json::json!("mock"));
        assert_eq!(json["table"], serde_json::json!("users"));
        assert!(
            json["filters"][0].as_str().unwrap().starts_with("id ="),
            "the condition reached the shell: {json}"
        );
        assert_eq!(json["order"], serde_json::json!("name desc"));
    }

    /// The security assertion: the preview engine carries the builder verbs and
    /// nothing else, so there is no way to spell a fetch on it.
    #[test]
    fn preview_engine_has_no_terminal_verbs() {
        for script in [
            r#"table("users").list()"#,
            r#"table("users").count()"#,
            r#"table("users").get_some()"#,
        ] {
            let err = preview_script(script.into(), resolver())
                .expect_err("a fetch verb must not exist on the preview engine");
            assert!(
                err.contains("Function not found"),
                "expected an unknown-function error for `{script}`, got: {err}"
            );
        }
    }

    #[test]
    fn preview_of_a_non_query_explains_itself() {
        let err = preview_script(r#""just a string""#.into(), resolver()).unwrap_err();
        assert!(
            err.contains("must end on the query itself"),
            "got: {err}"
        );
    }

    #[test]
    fn preview_traverses_a_reference_from_a_literal_row() {
        // No fetch anywhere: `get_ref` reads the join value out of the map it
        // is handed, so a drill-down previews from a made-up parent row.
        let json = preview_script(
            r#"table("users").get_ref("posts", #{ id: "1" })"#.into(),
            ref_resolver(),
        )
        .unwrap();
        assert_eq!(json["table"], serde_json::json!("posts"));
        assert!(
            json["filters"][0].as_str().unwrap().starts_with("author ="),
            "the join condition is visible: {json}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn syntax_error_is_reported() {
        let err = run_script("this is not rhai (".into(), resolver(), 5)
            .await
            .unwrap_err();
        assert!(!err.is_empty());
    }
}
