//! Who is making a write, carried to the table's lifecycle hooks.
//!
//! A host (an API server handling a request, a desktop app acting for its
//! user) wraps the work in [`WriteContext::scope`]; every write awaited inside
//! it hands the context to its hooks through their `context` field. Vantage
//! doesn't interpret it: `principal` is whatever record the host identifies the
//! actor with, and `group` ties together everything one request or action run
//! caused.

use std::future::Future;

tokio::task_local! {
    static CONTEXT: WriteContext;
}

/// Built with [`WriteContext::new`] and the `with_*` builders, so a field
/// added later (request data, say) doesn't break existing hosts.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct WriteContext {
    /// The actor, as a CBOR map (name plus attributes).
    pub principal: Option<ciborium::Value>,
    /// Shared by every write one request or action run causes.
    pub group: Option<String>,
}

impl WriteContext {
    /// An empty context: no principal, no group.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_principal(mut self, principal: ciborium::Value) -> Self {
        self.principal = Some(principal);
        self
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Run `fut` with this context in scope.
    pub async fn scope<F: Future>(self, fut: F) -> F::Output {
        CONTEXT.scope(self, fut).await
    }

    /// The context in scope for the current task, if any.
    pub fn current() -> Option<WriteContext> {
        CONTEXT.try_with(|c| c.clone()).ok()
    }
}
