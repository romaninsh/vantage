//! The typed failure of one `execute` call: what went wrong last, and how
//! many attempts were made before giving up.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// The server answered with a non-2xx status.
    Status(u16),
    /// No response at all: connect, timeout, TLS, body read.
    Transport(String),
    /// The circuit breaker was open and the policy said fail fast.
    BreakerOpen,
    /// The auth refresher returned an error.
    Auth(String),
    /// The client's semaphore was closed. Reserved for an explicit client
    /// shutdown; nothing closes the semaphore today.
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError {
    /// The failure of the last attempt.
    pub kind: ErrorKind,
    /// Attempts made, including the first one.
    pub attempts: usize,
    /// The start of the last non-2xx response body (up to 2 KiB, UTF-8
    /// lossy), for callers that surface the server's own message — an outbox
    /// entry's `last_error`, a lens log line. `None` when there was no
    /// response body to read, and for transport, breaker and auth failures.
    /// Not part of [`Display`](fmt::Display).
    pub body: Option<String>,
}

impl ClientError {
    pub(crate) fn new(kind: ErrorKind, attempts: usize) -> Self {
        Self {
            kind,
            attempts,
            body: None,
        }
    }

    pub(crate) fn with_body(mut self, body: Option<String>) -> Self {
        self.body = body;
        self
    }

    /// Whether no retry, now or later, could change this answer: a `4xx`
    /// other than `408` / `429`, an auth refresher that failed, or a closed
    /// client. Everything else (`5xx`, transport, an open breaker) is the
    /// transient kind a later call may get past, so callers log it quietly.
    pub fn is_final(&self) -> bool {
        match self.kind {
            ErrorKind::Status(s) => !super::policy::is_retryable_status(s),
            ErrorKind::Auth(_) | ErrorKind::Closed => true,
            ErrorKind::Transport(_) | ErrorKind::BreakerOpen => false,
        }
    }

    /// The HTTP status of the last attempt, when there was a response.
    pub fn status(&self) -> Option<u16> {
        match self.kind {
            ErrorKind::Status(s) => Some(s),
            _ => None,
        }
    }

    /// A stable lowercase name for the kind, for logs and tests.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            ErrorKind::Status(_) => "status",
            ErrorKind::Transport(_) => "transport",
            ErrorKind::BreakerOpen => "breaker_open",
            ErrorKind::Auth(_) => "auth",
            ErrorKind::Closed => "closed",
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = if self.attempts == 1 {
            "attempt"
        } else {
            "attempts"
        };
        match &self.kind {
            ErrorKind::Status(s) => write!(f, "HTTP {s} after {} {plural}", self.attempts),
            ErrorKind::Transport(e) => write!(f, "{e} after {} {plural}", self.attempts),
            ErrorKind::BreakerOpen => write!(f, "circuit breaker open"),
            ErrorKind::Auth(e) => write!(f, "auth refresh failed: {e}"),
            ErrorKind::Closed => write!(f, "client closed"),
        }
    }
}

impl std::error::Error for ClientError {}
