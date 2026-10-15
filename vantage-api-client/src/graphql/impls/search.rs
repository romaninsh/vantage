//! Quicksearch and "contains" patterns for the GraphQL adapter.
//!
//! Both render as `_ilike` with a `%…%` pattern. The user's text is escaped
//! first, so a `%` or `_` they typed matches itself rather than acting as a
//! wildcard.

use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::Entity;

use crate::graphql::api::GraphqlApi;
use crate::graphql::condition::GraphqlCondition;
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::types::AnyGraphqlType;

/// Escape the LIKE metacharacters (`\`, `%`, `_`) in `text` with a
/// backslash, Postgres' default escape character.
pub(crate) fn escape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `%<text>%` with `text` escaped — "contains", for `_ilike`.
pub(crate) fn contains_pattern(text: &str) -> String {
    format!("%{}%", escape_like(text))
}

/// A condition no row satisfies: `{_not: {}}` on Hasura, since `{}` is
/// true. Evaluated locally it is `!true` as well.
pub(crate) fn match_nothing() -> GraphqlCondition {
    GraphqlCondition::Not(Box::new(GraphqlCondition::And(Vec::new())))
}

/// Columns a quicksearch looks in: [`Table::search_columns`], those flagged
/// searchable or every column. When none is flagged, only string-typed
/// columns other than the id are kept, since `_ilike` takes text. Dotted
/// paths are left out either way: Hasura filters a nested field with a
/// nested object, which the renderer doesn't produce.
///
/// Which rule applies is settled before dotted paths are dropped, so a table
/// whose only searchable columns are dotted searches nothing rather than
/// widening to every string column.
fn search_columns<E: Entity<AnyGraphqlType>>(table: &Table<GraphqlApi, E>) -> Vec<String> {
    let columns = table.search_columns();
    let flagged = columns
        .iter()
        .any(|col| col.flags().contains(&ColumnFlag::Searchable));
    let id = table.id_field().map(|c| c.name().to_string());
    columns
        .into_iter()
        .filter(|col| flagged || col.get_type() == std::any::type_name::<String>())
        .filter(|col| flagged || Some(col.name()) != id.as_deref())
        .filter(|col| !col.name().contains('.'))
        .map(|col| col.name().to_string())
        .collect()
}

/// An OR of `_ilike '%text%'` over [`search_columns`]; with no column to
/// look in, a condition that matches nothing.
pub(crate) fn search_condition<E: Entity<AnyGraphqlType>>(
    table: &Table<GraphqlApi, E>,
    text: &str,
) -> GraphqlCondition {
    let pattern = contains_pattern(text);
    let mut conditions: Vec<GraphqlCondition> = search_columns(table)
        .iter()
        .map(|name| Column::<AnyGraphqlType>::new(name).ilike(pattern.clone()))
        .collect();
    match conditions.len() {
        0 => match_nothing(),
        1 => conditions.remove(0),
        _ => GraphqlCondition::Or(conditions),
    }
}

/// Undo [`escape_like`] on a pattern core, for local matching.
pub(crate) fn unescape_like(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}
