//! Model-level actions — typed business operations a model crate defines
//! once and every transport reuses.
//!
//! An action is three things: a serializable input, a serializable
//! output, and an async Rust body. The body lives on the model's tables
//! (extension-trait methods such as `Table<SurrealDB, Tag>::register`);
//! this crate provides the uniform wrapper around it — [`ModelAction`] —
//! that transports consume without knowing anything about the model:
//!
//! - an HTTP layer mounts it as a route: target id from the path, input
//!   deserialized from the request body (see `vantage-api-adapters`);
//! - a UI renders a form synthesized from [`ActionDescriptor::input_schema`]
//!   and invokes on confirm, filling the target id from the selected row;
//! - a scripting host (rhai) registers it as a host function taking one
//!   map argument and returning one map.
//!
//! Because the input type is one Rust struct (`Deserialize` +
//! `JsonSchema`), adding a field to it changes every surface at once —
//! the API accepts it, the UI form grows the input, scripts may pass it.
//! No transport declares fields of its own.
//!
//! The adapters here ([`record_action`], [`table_action`]) turn a typed
//! async closure into a `dyn ModelAction`. The [`actions`] attribute
//! generates calls to these adapters from trait method signatures; the
//! adapters are the foundation and remain public — anything the macro's
//! parameter classification cannot express is written against them
//! directly.

use std::future::Future;
use std::sync::Arc;

/// Generate [`ModelAction`] constructors from a trait's method
/// signatures — see the macro's own documentation for the parameter
/// roles it recognises.
///
/// Re-exported from the `vantage-action-macros` companion crate, which
/// exists only because a proc-macro crate cannot export ordinary items.
/// Depend on `vantage-action` alone and reach the attribute here.
pub use vantage_action_macros::actions;

use async_trait::async_trait;
use schemars::{JsonSchema, schema::RootSchema, schema_for};
use serde::Serialize;
use serde::de::DeserializeOwned;
use vantage_core::Result;

/// Where an action attaches, which decides how transports surface it:
/// a table action sits beside table-level operations (UI toolbar,
/// `POST /<table>/actions/<name>`), a record action targets one row
/// (UI row action, `POST /<table>/{id}/actions/<name>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionTarget {
    Table { table: String },
    Record { table: String },
}

impl ActionTarget {
    pub fn table(&self) -> &str {
        match self {
            ActionTarget::Table { table } | ActionTarget::Record { table } => table,
        }
    }
}

/// The target a transport resolved for one invocation. `Id` carries the
/// record key — bare (`GH123`) or backend-qualified (`tag:GH123`);
/// adapters strip the `<table>:` prefix so bodies always see the bare key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    None,
    Id(String),
}

/// What an invocation produced. `Value` is the common case — any
/// serialized result. `Record` points at a record the action created or
/// changed, letting transports react beyond display (UI opens/refreshes
/// it, HTTP returns its id, scripts get a handle).
#[derive(Debug, Clone)]
pub enum ActionOutput {
    Value(serde_json::Value),
    Record {
        table: String,
        id: String,
        value: serde_json::Value,
    },
}

impl ActionOutput {
    /// The JSON a transport serializes over the wire, whichever variant.
    pub fn into_value(self) -> serde_json::Value {
        match self {
            ActionOutput::Value(v) => v,
            ActionOutput::Record { value, .. } => value,
        }
    }
}

/// Everything a transport needs to surface an action without invoking
/// it: identity, placement, and the input/output contracts as JSON
/// Schema. The input schema is what UI form synthesis consumes.
#[derive(Debug, Clone)]
pub struct ActionDescriptor {
    pub name: String,
    pub target: ActionTarget,
    /// Human description — dialog body text, API documentation.
    pub description: String,
    pub input_schema: RootSchema,
    pub output_schema: RootSchema,
}

/// One action: serialized input in, serialized output out, async Rust
/// body in between. Object-safe so registries can hold a heterogeneous
/// set; typed construction goes through [`record_action`] /
/// [`table_action`].
#[async_trait]
pub trait ModelAction: Send + Sync {
    fn descriptor(&self) -> &ActionDescriptor;

    /// Run the action. `target` must match the descriptor:
    /// `Target::Id` for record actions, `Target::None` for table
    /// actions. `args` is the serialized input record.
    async fn invoke(&self, target: Target, args: serde_json::Value) -> Result<ActionOutput>;
}

/// Strip a `<table>:` prefix from a record key, so bodies receive the
/// bare key regardless of which transport supplied the id.
pub fn bare_key<'a>(table: &str, key: &'a str) -> &'a str {
    key.strip_prefix(table)
        .and_then(|rest| rest.strip_prefix(':'))
        .unwrap_or(key)
}

// ---------------------------------------------------------------------
// Typed adapters
// ---------------------------------------------------------------------

struct FnAction<F> {
    descriptor: ActionDescriptor,
    body: F,
}

type BoxedBody = Box<
    dyn Fn(
            Option<String>,
            serde_json::Value,
        ) -> std::pin::Pin<Box<dyn Future<Output = Result<ActionOutput>> + Send>>
        + Send
        + Sync,
>;

#[async_trait]
impl ModelAction for FnAction<BoxedBody> {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    async fn invoke(&self, target: Target, args: serde_json::Value) -> Result<ActionOutput> {
        let id = match (&self.descriptor.target, target) {
            (ActionTarget::Record { table }, Target::Id(id)) => {
                Some(bare_key(table, &id).to_string())
            }
            (ActionTarget::Record { .. }, Target::None) => {
                return Err(vantage_core::error!(
                    "record action requires a target id",
                    action = &self.descriptor.name
                )
                .mark_incorrect_usage());
            }
            (ActionTarget::Table { .. }, Target::None) => None,
            (ActionTarget::Table { .. }, Target::Id(_)) => {
                return Err(vantage_core::error!(
                    "table action does not take a target id",
                    action = &self.descriptor.name
                )
                .mark_incorrect_usage());
            }
        };
        (self.body)(id, args).await
    }
}

/// Deserialization failures are the caller's fault, so they are marked
/// [`ErrorKind::IncorrectUsage`](vantage_core::ErrorKind::IncorrectUsage)
/// — that is the signal a transport needs to answer 4xx without reading
/// the message text. Everything a body itself returns stays unclassified
/// and is treated as a server-side failure.
fn deserialize_input<A: DeserializeOwned>(name: &str, args: serde_json::Value) -> Result<A> {
    serde_json::from_value(args).map_err(|e| {
        vantage_core::error!(
            "invalid action input",
            action = name,
            details = e.to_string()
        )
        .mark_incorrect_usage()
    })
}

fn serialize_output<R: Serialize>(name: &str, out: R) -> Result<ActionOutput> {
    let value = serde_json::to_value(out).map_err(|e| {
        vantage_core::error!(
            "action output failed to serialize",
            action = name,
            details = e.to_string()
        )
    })?;
    Ok(ActionOutput::Value(value))
}

/// Wrap a typed async closure as a record-targeted [`ModelAction`].
/// The closure receives the bare record key and the deserialized input;
/// its output serializes to [`ActionOutput::Value`].
pub fn record_action<A, R, F, Fut>(
    name: &str,
    table: &str,
    description: &str,
    f: F,
) -> Arc<dyn ModelAction>
where
    A: DeserializeOwned + JsonSchema + Send + 'static,
    R: Serialize + JsonSchema + 'static,
    F: Fn(String, A) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    let descriptor = ActionDescriptor {
        name: name.to_string(),
        target: ActionTarget::Record {
            table: table.to_string(),
        },
        description: description.to_string(),
        input_schema: schema_for!(A),
        output_schema: schema_for!(R),
    };
    let action_name = descriptor.name.clone();
    let body: BoxedBody = Box::new(move |id, args| {
        // Record actions always receive Some(id) — enforced by invoke().
        let id = id.expect("record action invoked without id");
        let name = action_name.clone();
        let fut = deserialize_input::<A>(&name, args).map(|input| f(id, input));
        Box::pin(async move { serialize_output(&name, fut?.await?) })
    });
    Arc::new(FnAction { descriptor, body })
}

/// Wrap a typed async closure as a table-targeted [`ModelAction`].
pub fn table_action<A, R, F, Fut>(
    name: &str,
    table: &str,
    description: &str,
    f: F,
) -> Arc<dyn ModelAction>
where
    A: DeserializeOwned + JsonSchema + Send + 'static,
    R: Serialize + JsonSchema + 'static,
    F: Fn(A) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    let descriptor = ActionDescriptor {
        name: name.to_string(),
        target: ActionTarget::Table {
            table: table.to_string(),
        },
        description: description.to_string(),
        input_schema: schema_for!(A),
        output_schema: schema_for!(R),
    };
    let action_name = descriptor.name.clone();
    let body: BoxedBody = Box::new(move |_id, args| {
        let name = action_name.clone();
        let fut = deserialize_input::<A>(&name, args).map(&f);
        Box::pin(async move { serialize_output(&name, fut?.await?) })
    });
    Arc::new(FnAction { descriptor, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, JsonSchema)]
    struct Greet {
        name: String,
        #[serde(default)]
        shout: Option<bool>,
    }

    #[derive(Serialize, JsonSchema)]
    struct Greeted {
        message: String,
    }

    fn greet_action() -> Arc<dyn ModelAction> {
        record_action(
            "greet",
            "person",
            "Greets a person.",
            |id, g: Greet| async move {
                let mut message = format!("hello {} ({id})", g.name);
                if g.shout.unwrap_or(false) {
                    message = message.to_uppercase();
                }
                Ok(Greeted { message })
            },
        )
    }

    #[tokio::test]
    async fn record_action_strips_table_prefix_and_round_trips() {
        let action = greet_action();
        assert_eq!(
            action.descriptor().target,
            ActionTarget::Record {
                table: "person".into()
            }
        );
        let out = action
            .invoke(
                Target::Id("person:42".into()),
                serde_json::json!({"name": "ada"}),
            )
            .await
            .unwrap();
        assert_eq!(
            out.into_value(),
            serde_json::json!({"message": "hello ada (42)"})
        );
    }

    #[tokio::test]
    async fn record_action_requires_an_id() {
        let err = greet_action()
            .invoke(Target::None, serde_json::json!({"name": "ada"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("requires a target id"), "{err}");
    }

    #[tokio::test]
    async fn bad_input_is_a_clear_error_not_a_panic() {
        let err = greet_action()
            .invoke(Target::Id("42".into()), serde_json::json!({"shout": true}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid action input"), "{err}");
    }

    #[test]
    fn input_schema_reflects_optionality() {
        let action = greet_action();
        let schema = serde_json::to_value(&action.descriptor().input_schema).unwrap();
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "name"));
        assert!(!required.iter().any(|v| v == "shout"));
    }

    #[test]
    fn bare_key_tolerates_both_spellings() {
        assert_eq!(bare_key("tag", "tag:GH123"), "GH123");
        assert_eq!(bare_key("tag", "GH123"), "GH123");
        // A key whose content merely starts with the table name is untouched.
        assert_eq!(bare_key("tag", "taggy"), "taggy");
    }
}
