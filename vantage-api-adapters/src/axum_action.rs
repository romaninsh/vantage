//! Axum transport for [`ModelAction`]s — mount a model's action set as
//! HTTP routes with zero per-action code.
//!
//! Each action's descriptor says what the route needs: a `Record`
//! target gets `POST /{table}/{id}/actions/{name}` (the `#[id]`
//! parameter auto-filled from the path), a `Table` target gets
//! `POST /{table}/actions/{name}`. The request body deserializes into
//! the action's input type inside `invoke` — a wrong-shaped body is a
//! field-level 400 before any business logic runs.
//!
//! Legacy paths stay one-liners: [`ActionRouter::alias`] returns a
//! handler for any `…/{id}/…` route an existing contract requires:
//!
//! ```ignore
//! let actions = ActionRouter::new(chtags_model::model_actions(db, mailer))?;
//! let app = Router::new()
//!     .route("/tag/{id}/register", actions.alias("tag", "register_tag")?)
//!     .merge(actions.into_router());
//! ```
//!
//! `alias` already returns a `MethodRouter`, so it goes straight into
//! `route` — no `post(…)` wrapper.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, post};
use axum::{Json, Router};
use vantage_action::{ActionOutput, ActionTarget, ModelAction, Target};
use vantage_core::VantageError;

use crate::axum_dio::ApiError;

/// Maps model errors onto HTTP statuses by classification, not by
/// message text.
///
/// [`ErrorKind::NotFound`](vantage_core::ErrorKind::NotFound) is 404 and
/// [`ErrorKind::IncorrectUsage`](vantage_core::ErrorKind::IncorrectUsage)
/// — a malformed input, a record action invoked without an id — is 400.
/// Those are the only two answered 4xx. Everything else is a failure the
/// caller cannot fix by retrying differently (a database that is down,
/// an outbox that rejected the message), so it is a 500 and is logged.
///
/// Answering 4xx for an outage is worse than a wrong number: it tells
/// every client, retry policy and alerting rule that nothing is broken.
/// A model that wants a specific status marks its error accordingly;
/// this function never reads the message text.
fn error_response(e: VantageError) -> ApiError {
    let status = if e.is_not_found() {
        StatusCode::NOT_FOUND
    } else if e.is_incorrect_usage() {
        StatusCode::BAD_REQUEST
    } else {
        // Match `axum_dio`'s conversion: a failure that reaches the
        // transport has to leave a server-side trace, or an outage looks
        // exactly like ordinary bad input in the logs.
        tracing::error!(error = ?e, "model action failed");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    ApiError {
        status,
        message: e.to_string(),
    }
}

async fn run(
    action: Arc<dyn ModelAction>,
    target: Target,
    body: serde_json::Value,
) -> axum::response::Response {
    match action.invoke(target, body).await {
        Ok(ActionOutput::Value(v)) => Json(v).into_response(),
        Ok(ActionOutput::Record { value, .. }) => Json(value).into_response(),
        Err(e) => error_response(e).into_response(),
    }
}

/// A set of [`ModelAction`]s ready to mount. Construct once from the
/// model's action list; `into_router()` yields the canonical routes and
/// `alias()` hands out per-action handlers for legacy paths.
pub struct ActionRouter {
    /// Keyed by `(table, name)`. An action's identity is both — two
    /// models may each expose `archive`, and their routes (`/tag/…` and
    /// `/user/…`) do not collide, so neither may their registrations.
    actions: HashMap<(String, String), Arc<dyn ModelAction>>,
}

impl ActionRouter {
    /// Errors on two actions sharing a table and a name — silently
    /// dropping one would leave a route missing with nothing to explain
    /// why.
    pub fn new(actions: Vec<Arc<dyn ModelAction>>) -> vantage_core::Result<Self> {
        let mut map: HashMap<(String, String), Arc<dyn ModelAction>> = HashMap::new();
        for action in actions {
            let descriptor = action.descriptor();
            let key = (
                descriptor.target.table().to_string(),
                descriptor.name.clone(),
            );
            if map.contains_key(&key) {
                return Err(vantage_core::error!(
                    "two actions share a table and a name",
                    table = &key.0,
                    action = &key.1
                )
                .mark_incorrect_usage());
            }
            map.insert(key, action);
        }
        Ok(Self { actions: map })
    }

    /// The canonical routes, one per action, derived from descriptors.
    ///
    /// Generic over the app's state type: the handlers hold every
    /// dependency in the action itself and read no state, so this merges
    /// into a `Router<AppState>` as readily as a `Router<()>`.
    pub fn into_router<S>(self) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        let mut router = Router::new();
        for ((_, name), action) in self.actions {
            let path = match &action.descriptor().target {
                ActionTarget::Record { table } => format!("/{table}/{{id}}/actions/{name}"),
                ActionTarget::Table { table } => format!("/{table}/actions/{name}"),
            };
            router = router.route(&path, Self::method_router(action));
        }
        router
    }

    /// A handler for mounting one action on a legacy path of your own
    /// choosing. Record actions expect an `{id}` segment in that path;
    /// table actions expect none. Errors on an unknown table/name pair so
    /// a typo fails at startup, not on first request.
    pub fn alias<S>(&self, table: &str, name: &str) -> vantage_core::Result<MethodRouter<S>>
    where
        S: Clone + Send + Sync + 'static,
    {
        let action = self
            .actions
            .get(&(table.to_string(), name.to_string()))
            .cloned()
            .ok_or_else(|| {
                vantage_core::error!("unknown action", table = table, action = name)
                    .mark_incorrect_usage()
            })?;
        Ok(Self::method_router(action))
    }

    fn method_router<S>(action: Arc<dyn ModelAction>) -> MethodRouter<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        match &action.descriptor().target {
            // `Path<HashMap<..>>` rather than `Path<String>`: an alias
            // path may carry more captures than the id (a tenant prefix,
            // say), and `Path<String>` answers those with a 500 that
            // leaks axum internals.
            ActionTarget::Record { .. } => post(
                move |Path(params): Path<HashMap<String, String>>,
                      Json(body): Json<serde_json::Value>| async move {
                    match params.get("id") {
                        Some(id) => run(action, Target::Id(id.clone()), body).await,
                        None => ApiError {
                            status: StatusCode::INTERNAL_SERVER_ERROR,
                            message: format!(
                                "route for record action `{}` has no `{{id}}` segment",
                                action.descriptor().name
                            ),
                        }
                        .into_response(),
                    }
                },
            ),
            ActionTarget::Table { .. } => {
                post(move |Json(body): Json<serde_json::Value>| async move {
                    run(action, Target::None, body).await
                })
            }
        }
    }
}
