//! [`LoadFailure`]: why a view shows no rows for its query.

use std::fmt;

/// The clause a [`LoadFailure`] names: a condition or the order, on its
/// column, or the search.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Clause {
    Filter(String),
    Order(String),
    Search,
}

impl fmt::Display for Clause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Clause::Filter(column) => write!(f, "filter on {column}"),
            Clause::Order(column) => write!(f, "order on {column}"),
            Clause::Search => f.write_str("search"),
        }
    }
}

/// Why a view has no honest answer for its current query, carried by
/// [`LoadState::Failed`](super::LoadState::Failed).
///
/// A view that pages windows from its master can only show what the master
/// applied. When the master rejects a clause, or refuses one that the view
/// has no complete set to evaluate over, the view withholds its rows rather
/// than present a window as the filtered set. `clause` names the clause when
/// it is known.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoadFailure {
    /// The master table the view reads.
    pub table: String,
    pub clause: Option<Clause>,
    pub message: String,
}

impl LoadFailure {
    pub(crate) fn new(table: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            clause: None,
            message: message.into(),
        }
    }

    pub(crate) fn naming(mut self, clause: Clause) -> Self {
        self.clause = Some(clause);
        self
    }
}

impl fmt::Display for LoadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.table, self.message)?;
        match &self.clause {
            Some(clause) => write!(f, " ({clause})"),
            None => Ok(()),
        }
    }
}
