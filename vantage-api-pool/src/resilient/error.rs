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
    /// The client's semaphore was closed (the client is being torn down).
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientError {
    /// The failure of the last attempt.
    pub kind: ErrorKind,
    /// Attempts made, including the first one.
    pub attempts: usize,
}

impl ClientError {
    pub(crate) fn new(kind: ErrorKind, attempts: usize) -> Self {
        Self { kind, attempts }
    }

    /// The HTTP status of the last attempt, when there was a response.
    pub fn status(&self) -> Option<u16> {
        match self.kind {
            ErrorKind::Status(s) => Some(s),
            _ => None,
        }
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = if self.attempts == 1 { "attempt" } else { "attempts" };
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
