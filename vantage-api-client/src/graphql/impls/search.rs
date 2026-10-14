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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vantage_types::EmptyEntity;

    use crate::graphql::condition::{FilterDialect, GraphqlOp};

    fn api() -> GraphqlApi {
        GraphqlApi::new("https://api.test/graphql")
    }

    #[test]
    fn metacharacters_are_escaped() {
        assert_eq!(contains_pattern(r"50%_a\b"), r"%50\%\_a\\b%");
        assert_eq!(unescape_like(r"50\%\_a\\b"), r"50%_a\b");
    }

    #[test]
    fn typed_tables_search_their_string_columns_but_not_the_id() {
        let table = Table::<GraphqlApi, EmptyEntity>::new("launches", api())
            .with_id_column("id")
            .with_column_of::<String>("mission_name")
            .with_column_of::<i64>("launch_year")
            .with_column_of::<String>("rocket.name");
        assert_eq!(search_columns(&table), ["mission_name"]);
    }

    #[test]
    fn searchable_flags_take_precedence() {
        let mut table = Table::<GraphqlApi, EmptyEntity>::new("launches", api())
            .with_id_column("id")
            .with_column_of::<String>("mission_name");
        table
            .add_column(Column::<AnyGraphqlType>::new("details").with_flag(ColumnFlag::Searchable));
        assert_eq!(search_columns(&table), ["details"]);
    }

    /// A searchable flag only on a dotted column must not fall back to
    /// every string column: there is nothing to search in.
    #[tokio::test]
    async fn dotted_only_searchable_flags_match_nothing() {
        let mut table = Table::<GraphqlApi, EmptyEntity>::new("launches", api())
            .with_id_column("id")
            .with_column_of::<String>("mission_name");
        table.add_column(
            Column::<AnyGraphqlType>::new("rocket.name").with_flag(ColumnFlag::Searchable),
        );
        assert!(search_columns(&table).is_empty());
        assert_eq!(
            search_condition(&table, "fal")
                .render(FilterDialect::Hasura)
                .await
                .unwrap(),
            json!({ "_not": {} })
        );
    }

    #[tokio::test]
    async fn no_searchable_column_matches_nothing() {
        let table = Table::<GraphqlApi, EmptyEntity>::new("launches", api())
            .with_id_column("id")
            .with_column_of::<i64>("launch_year");
        let cond = search_condition(&table, "falcon");
        assert_eq!(
            cond.render(FilterDialect::Hasura).await.unwrap(),
            json!({ "_not": {} })
        );
    }

    #[test]
    fn a_single_column_search_is_one_ilike() {
        let table = Table::<GraphqlApi, EmptyEntity>::new("launches", api())
            .with_id_column("id")
            .with_column_of::<String>("mission_name");
        match search_condition(&table, "fal_con") {
            GraphqlCondition::Field(fc) => {
                assert_eq!(fc.op, GraphqlOp::ILike);
                assert_eq!(fc.value, json!(r"%fal\_con%"));
            }
            other => panic!("expected one ilike, got {other:?}"),
        }
    }
}
