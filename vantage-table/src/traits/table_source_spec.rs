/// How a `TableSource` names the place its rows come from.
///
/// Most backends use a plain table/collection name (`String`). SQL and
/// SurrealDB backends use [`crate::source::SelectSource`], which is either a
/// name or an arbitrary sub-`SELECT`. `Table::table_name()` returns
/// [`name`](TableSourceSpec::name) so the ~hundreds of existing callers keep
/// working regardless of which source kind a backend chose.
pub trait TableSourceSpec: Clone + Send + Sync + 'static {
    /// The source's name. For a query source this is its FROM alias, which is
    /// also the SQL prefix used when the table participates in correlated
    /// subqueries.
    fn name(&self) -> &str;

    /// Build a name-based source. Used by `Table::new` and the REST drivers'
    /// `set_table_name`.
    fn from_name(name: String) -> Self;
}

impl TableSourceSpec for String {
    fn name(&self) -> &str {
        self.as_str()
    }

    fn from_name(name: String) -> Self {
        name
    }
}
