//! Source representation for tables whose FROM clause may be a sub-`SELECT`.
//!
//! A backend's [`TableSource::Source`](crate::traits::table_source::TableSource::Source)
//! is either a plain `String` (most backends) or [`SelectSource<S>`] — the
//! shared enum used by every backend whose `Select` can express
//! `FROM (subquery)` (SQLite, PostgreSQL, MySQL, SurrealDB).

use vantage_expressions::{Expressive, expr_any, traits::selectable::Selectable};

use crate::traits::table_source_spec::TableSourceSpec;

/// The source of a table: a named table, or an arbitrary query used as a
/// derived (sub-`SELECT`) source.
///
/// `S` is the backend's `Select` type. The same enum serves all four
/// subquery-capable backends; the only per-backend code is the one-line
/// `type Source = SelectSource<Self::Select>;` on their `TableSource` impl.
#[derive(Clone, Debug)]
pub enum SelectSource<S> {
    /// A physical table/collection name.
    Name(String),
    /// A query used as the FROM source, rendered `FROM (<select>) AS <alias>`.
    Query { select: Box<S>, alias: String },
}

impl<S> SelectSource<S> {
    /// Build a query source from a select and the alias to expose it under.
    pub fn query(select: S, alias: impl Into<String>) -> Self {
        SelectSource::Query {
            select: Box::new(select),
            alias: alias.into(),
        }
    }
}

impl<S: Clone + Send + Sync + 'static> TableSourceSpec for SelectSource<S> {
    fn name(&self) -> &str {
        match self {
            SelectSource::Name(name) => name.as_str(),
            SelectSource::Query { alias, .. } => alias.as_str(),
        }
    }

    fn from_name(name: String) -> Self {
        SelectSource::Name(name)
    }
}

impl<S> From<&str> for SelectSource<S> {
    fn from(value: &str) -> Self {
        SelectSource::Name(value.to_string())
    }
}

impl<S> From<String> for SelectSource<S> {
    fn from(value: String) -> Self {
        SelectSource::Name(value)
    }
}

/// Applies a source to a freshly-created `Select`.
///
/// `Table::select_empty` is generic over *every* `SelectableDataSource`,
/// including backends whose `Source` is `String` and whose `Select` is not
/// `Expressive`. So the source can't be matched against `SelectSource`
/// directly there — this trait dispatches on the concrete source type, keeping
/// the `Expressive` requirement confined to the query-capable backends.
pub trait SelectSeed<S, V, C> {
    /// Add this source to `select` as its FROM clause.
    fn seed(&self, select: &mut S)
    where
        S: Selectable<V, C>,
        V: From<String>;
}

impl<S, V, C> SelectSeed<S, V, C> for String {
    fn seed(&self, select: &mut S)
    where
        S: Selectable<V, C>,
        V: From<String>,
    {
        select.add_source(self.as_str(), None);
    }
}

impl<S, V, C> SelectSeed<S, V, C> for SelectSource<S>
where
    S: Expressive<V> + Clone,
    V: Clone,
{
    fn seed(&self, select: &mut S)
    where
        S: Selectable<V, C>,
        V: From<String>,
    {
        match self {
            SelectSource::Name(name) => select.add_source(name.as_str(), None),
            SelectSource::Query {
                select: query,
                alias,
            } => {
                // Parenthesize the subquery ourselves: `expr()` renders a bare
                // statement, and the SQL dialects' `add_source` does not wrap a
                // nested source (only SurrealDB does). One wrap here is correct
                // for every backend.
                let subquery = expr_any!("({})", (query.expr()));
                select.add_source(subquery, Some(alias.clone()));
            }
        }
    }
}
