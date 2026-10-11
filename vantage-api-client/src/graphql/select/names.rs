//! The names a `GraphqlSelect` splices into its document, checked against
//! the GraphQL `Name` grammar before anything is rendered.

use serde_json::Value;
use vantage_core::Result;

use crate::graphql::condition::FilterDialect;
use crate::graphql::name::{validate, validate_object_keys, validate_path, validate_root_args};
use crate::graphql::select::GraphqlSelect;
use crate::graphql::select::render::json_to_graphql_value;

impl GraphqlSelect {
    /// The filter argument's name: the override, else `where` (Hasura) or
    /// `find` (Generic).
    pub(crate) fn filter_arg(&self) -> &str {
        self.filter_arg_name
            .as_deref()
            .unwrap_or(match self.dialect {
                FilterDialect::Hasura => "where",
                FilterDialect::Generic => "find",
            })
    }

    /// Check the names in the selection: operation name, fields (dotted
    /// paths segment by segment), column aliases, response envelope, order
    /// fields, and the same for every sub-selection.
    pub(crate) fn validate_selection_names(&self) -> Result<()> {
        if let Some(op) = self.operation_name.as_deref().filter(|n| !n.is_empty()) {
            validate("operation name", op)?;
        }
        for field in &self.fields {
            validate_path("field", field)?;
        }
        for (column, field) in &self.aliases {
            validate("field", column)?;
            validate("field", field)?;
        }
        for segment in &self.response_path {
            validate("response path segment", segment)?;
        }
        for (field, _) in &self.sort {
            validate("order field", field)?;
        }
        for (field, child) in &self.sub_selections {
            validate("field", field)?;
            child.validate_selection_names()?;
        }
        Ok(())
    }

    /// `name: value` for each literal root argument, keys checked at every
    /// depth.
    pub(crate) fn render_root_args(&self) -> Result<Vec<String>> {
        let Some(args @ Value::Object(map)) = &self.root_args else {
            return Ok(Vec::new());
        };
        validate_root_args(args)?;
        Ok(map
            .iter()
            .map(|(name, value)| format!("{}: {}", name, json_to_graphql_value(value)))
            .collect())
    }

    /// `<filter arg>: <rendered condition>`, the argument name and every
    /// key of the condition object checked.
    pub(crate) fn render_filter_arg(&self, condition: &Value) -> Result<String> {
        let arg = self.filter_arg();
        validate("filter argument", arg)?;
        validate_object_keys("filter key", condition)?;
        Ok(format!("{}: {}", arg, json_to_graphql_value(condition)))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vantage_expressions::Order;

    use crate::graphql::condition::{FieldCondition, GraphqlCondition, GraphqlOp};
    use crate::graphql::select::GraphqlSelect;

    fn base() -> GraphqlSelect {
        GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("id")
    }

    async fn rejected(select: GraphqlSelect, kind: &str) {
        let err = select.render().await.unwrap_err().to_string();
        assert!(err.contains("GraphQL name is not valid"), "{err}");
        assert!(err.contains(kind), "{kind}: {err}");
    }

    #[tokio::test]
    async fn every_interpolated_name_is_checked() {
        rejected(base().with_root_field("launches { secret }"), "root field").await;
        rejected(base().with_field("run.id }"), "field").await;
        rejected(base().with_operation_name("Q { x }"), "operation name").await;
        rejected(
            base().with_response_path(vec!["edges".into(), "no de".into()]),
            "response path segment",
        )
        .await;
        rejected(base().with_order("id: asc}, {x", Order::Asc), "order field").await;
        rejected(
            base()
                .with_filter_arg_name("find)")
                .with_condition(GraphqlCondition::eq("id", "1")),
            "filter argument",
        )
        .await;
        rejected(
            base().with_root_args(json!({ "in put": {} })),
            "root argument",
        )
        .await;
        rejected(
            base().with_root_args(json!({ "input": { "a}": 1 } })),
            "root argument key",
        )
        .await;
        rejected(
            base().with_sub_selection("rocket x", GraphqlSelect::new().with_field("id")),
            "field",
        )
        .await;
    }

    #[tokio::test]
    async fn condition_fields_are_checked_as_filter_keys() {
        let select = base()
            .with_filter_arg_name("find")
            .with_condition(GraphqlCondition::Field(FieldCondition::new(
                "name}) { id } q: launches(find: {x",
                GraphqlOp::Eq,
                json!("v"),
            )));
        rejected(select, "filter key").await;
    }

    #[tokio::test]
    async fn the_aggregate_count_checks_its_field_and_filter() {
        let err = base()
            .render_aggregate_count("users_aggregate { x }")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("count field"), "{err}");

        let err = base()
            .with_condition(GraphqlCondition::eq("a b", "v"))
            .render_aggregate_count("users_aggregate")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("filter key"), "{err}");
    }
}
