//! Model-level actions: typed business operations a model crate defines
//! once and every transport (HTTP route, UI form, script host) reuses
//! through the uniform [`ModelAction`] wrapper.
//!
//! The input type is one Rust struct (`Deserialize` + `JsonSchema`), so
//! adding a field changes every surface at once. [`record_action`] and
//! [`table_action`] turn a typed async closure into a `dyn ModelAction`;
//! the [`actions`] attribute generates calls to them from trait methods.

use std::future::Future;
use std::sync::Arc;

/// Generate [`ModelAction`] constructors from a trait's method
/// signatures — see the macro's own documentation for the parameter
/// roles it recognises.
///
/// Re-exported from the `vantage-action-macros` companion crate.
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
        // invoke() rejects record actions without an id before this runs.
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
