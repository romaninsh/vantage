//! Hasura-style aggregate count:
//! `query { <field>(where: …) { aggregate { count } } }`.
//!
//! The aggregate field takes the same narrowing arguments as the list
//! field it counts, so the select's root arguments and filter carry over
//! verbatim. Ordering, paging, the selection set and any `response_path`
//! envelope describe rows, not a count, and are left out.

use serde_json::Map;
use vantage_core::Result;

use crate::graphql::name::validate;
use crate::graphql::select::GraphqlSelect;
use crate::graphql::select::render::RenderedQuery;

impl GraphqlSelect {
    /// Render a count of the rows this select would list, queried through
    /// the aggregate root field `count_field`.
    /// `count_field` and every argument name are checked against the
    /// GraphQL `Name` grammar first.
    pub(crate) async fn render_aggregate_count(&self, count_field: &str) -> Result<RenderedQuery> {
        validate("count field", count_field)?;
        let args = self.render_root_and_filter_args().await?;
        let args_str = if args.is_empty() {
            String::new()
        } else {
            format!("({})", args.join(", "))
        };
        Ok(RenderedQuery {
            query: format!("query {{ {count_field}{args_str} {{ aggregate {{ count }} }} }}"),
            variables: Map::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::graphql::condition::{FieldCondition, FilterDialect, GraphqlCondition, GraphqlOp};
    use crate::graphql::select::GraphqlSelect;

    #[tokio::test]
    async fn count_keeps_the_filter_and_drops_paging_and_order() {
        let select = GraphqlSelect::new()
            .with_root_field("users")
            .with_field("id")
            .with_dialect(FilterDialect::Hasura)
            .with_condition(GraphqlCondition::Field(FieldCondition::new(
                "age",
                GraphqlOp::Gt,
                json!(30),
            )))
            .with_order("id", vantage_expressions::Order::Asc)
            .with_limit(Some(10), Some(20));
        let q = select
            .render_aggregate_count("users_aggregate")
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { users_aggregate(where: {age: {_gt: 30}}) { aggregate { count } } }"
        );
        assert!(q.variables.is_empty());
    }
}
