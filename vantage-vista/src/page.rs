//! What reads return beyond plain rows: [`Page`], [`Preview`] and the opaque
//! [`NativeCondition`] a caller hands to a shell.

use std::any::Any;
use std::fmt;

use ciborium::Value as CborValue;
use serde::Serialize;
use vantage_types::Record;

/// One window of rows, in the set's order, and the set's total row count
/// when the fetch learned it. `None` means the fetch didn't say, never zero.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct Page {
    pub rows: Vec<(String, Record<CborValue>)>,
    pub total: Option<i64>,
}

impl Page {
    pub fn new(rows: Vec<(String, Record<CborValue>)>) -> Self {
        Self { rows, total: None }
    }

    pub fn with_total(mut self, total: i64) -> Self {
        self.total = Some(total);
        self
    }
}

/// The query a shell would send for its next read, rendered without sending
/// it. A debugging surface: it must not show a clause the shell wouldn't
/// send, and a preview that leaves a detail out says so in `note`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Preview {
    /// The driver, as [`TableShell::driver_name`](crate::TableShell::driver_name).
    pub driver: &'static str,
    /// The native query (SQL, a GraphQL document, a Mongo filter, an argv);
    /// `None` when nothing is queried.
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Preview {
    pub fn new(driver: &'static str) -> Self {
        Self {
            driver,
            query: None,
            note: None,
        }
    }

    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// A condition in a driver's own type (a SurrealDB expression, a BSON
/// document), passed through vista unread, with a `key` naming it.
///
/// The key is what [`VistaQuery`](crate::VistaQuery) records for the
/// condition and what [`VistaQuery::key`](crate::VistaQuery::key) includes,
/// so two Vistas narrowed by different native conditions get different cache
/// identities. Give equal conditions equal keys, typically the rendered
/// expression.
///
/// The driver downcasts the condition to the type it takes. A condition of
/// another type is a caller mistake, answered with an `IncorrectUsage`
/// error, not `Unsupported`.
pub struct NativeCondition {
    condition: Box<dyn Any + Send + Sync>,
    key: String,
}

impl NativeCondition {
    pub fn new<C: Any + Send + Sync>(condition: C, key: impl Into<String>) -> Self {
        Self {
            condition: Box::new(condition),
            key: key.into(),
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// The condition as `C`, or `self` back when it holds another type.
    pub fn downcast<C: Any>(self) -> Result<C, Self> {
        let Self { condition, key } = self;
        condition
            .downcast::<C>()
            .map(|c| *c)
            .map_err(|condition| Self { condition, key })
    }
}

impl fmt::Debug for NativeCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("NativeCondition").field(&self.key).finish()
    }
}
