//! Which columns a query reads: a condition, the search or an order.

use super::{MemoryCondition, Query};

/// Whether `q` reads any of `columns` (or a dotted path under one). A search
/// over every cell reads them all.
pub(crate) fn reads_columns(q: &Query, columns: &[&str]) -> bool {
    if columns.is_empty() {
        return false;
    }
    let named = |path: &str| {
        columns.iter().any(|column| {
            path.strip_prefix(column)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
    };
    let searched = |cells: &[String]| cells.is_empty() || cells.iter().any(|c| named(c));
    q.conditions.iter().any(|c| reads(c, &named, &searched))
        || q.order.iter().any(|(path, _)| named(path))
        || (q.search.is_some() && searched(&q.search_columns))
}

fn reads(
    c: &MemoryCondition,
    named: &impl Fn(&str) -> bool,
    searched: &impl Fn(&[String]) -> bool,
) -> bool {
    match c {
        MemoryCondition::Cmp { path, .. }
        | MemoryCondition::Column(path)
        | MemoryCondition::DeferredIn { path, .. } => named(path),
        MemoryCondition::Search { columns, .. } => searched(columns),
        MemoryCondition::And(all) | MemoryCondition::Or(all) => {
            all.iter().any(|c| reads(c, named, searched))
        }
        MemoryCondition::Not(inner) => reads(inner, named, searched),
        MemoryCondition::Deferred(_) => false,
    }
}
