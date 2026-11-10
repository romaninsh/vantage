//! Who is making a write, carried to the table's lifecycle hooks.
//!
//! A host (an API server handling a request, a desktop app acting for its
//! user) wraps the work in [`WriteContext::scope`]; every write awaited inside
//! it hands the context to its hooks through their `context` field. Vantage
//! doesn't interpret it: `principal` is whatever record the host identifies the
//! actor with, and `group` ties together everything one request or action run
//! caused.

use std::future::Future;
use std::net::IpAddr;

use indexmap::IndexMap;

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
    /// The request the write came in on, filled by the host at its entry point.
    pub request: Option<RequestContext>,
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

    pub fn with_request(mut self, request: RequestContext) -> Self {
        self.request = Some(request);
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

/// Headers a host may copy into a [`RequestContext`]. Lowercase names.
pub const REQUEST_HEADER_ALLOW_LIST: &[&str] = &[
    "user-agent",
    "referer",
    "accept-language",
    "x-posthog-distinct-id",
    "x-posthog-session-id",
];

/// The entry point a write came in through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Ws,
    Rest,
    Mcp,
    Internal,
}

impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Transport::Ws => "ws",
            Transport::Rest => "rest",
            Transport::Mcp => "mcp",
            Transport::Internal => "internal",
        }
    }
}

/// What the host saw of the request a write came in on. Built with
/// [`RequestContext::new`] and the `with_*` builders.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct RequestContext {
    pub transport: Transport,
    pub ip: Option<IpAddr>,
    /// Keys are stored lowercase, so [`RequestContext::header`] ignores case.
    pub headers: IndexMap<String, String>,
    pub cookies: IndexMap<String, String>,
}

impl RequestContext {
    pub fn new(transport: Transport) -> Self {
        Self {
            transport,
            ip: None,
            headers: IndexMap::new(),
            cookies: IndexMap::new(),
        }
    }

    pub fn with_ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(name.into().to_ascii_lowercase(), value.into());
        self
    }

    pub fn with_cookie(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.cookies.insert(name.into(), value.into());
        self
    }

    /// The header's value; the name is matched case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(String::as_str)
    }
}
