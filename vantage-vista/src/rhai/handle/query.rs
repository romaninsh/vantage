//! A handle's narrowing as one [`VistaQuery`], for a consumer that re-applies
//! it elsewhere, such as a quick filter deriving a Dio with
//! `dio.derive().with_query(&q).build()`.

use vantage_core::{Result, error};

use super::{Handle, Step};
use crate::query::{Condition, VistaQuery};

impl Handle {
    /// The handle's `where`, `search` and `sort` steps folded into one
    /// [`VistaQuery`]: conditions in the order they were added, and the last
    /// `search` and the last `sort`, as resolving the handle would apply them.
    ///
    /// Steps with no query form are an incorrect-usage error naming the verb:
    /// `ref` moves to another table, `limit` caps a read rather than
    /// narrowing the set, and an extension verb's native condition has no
    /// form a consumer could re-apply. The handle's base is not included.
    pub fn query(&self) -> Result<VistaQuery> {
        if let Some(verb) = &self.native_verb {
            return Err(no_query_form(verb));
        }
        let mut query = VistaQuery::new();
        for step in self.steps.iter() {
            match step {
                Step::Where { col, op, value } => {
                    query = query.with_condition(Condition::new(col.as_str(), *op, value.clone()));
                }
                Step::Search(text) => query = query.with_search(text.as_str()),
                Step::Sort { col, dir } => query = query.with_order(col.as_str(), *dir),
                Step::Limit(_) => return Err(no_query_form("limit")),
                Step::Ref(_) => return Err(no_query_form("ref")),
            }
        }
        Ok(query)
    }
}

fn no_query_form(verb: &str) -> vantage_core::VantageError {
    error!(
        "Step has no query form; Handle::query() takes where, search and sort",
        verb = verb
    )
    .mark_incorrect_usage()
}
