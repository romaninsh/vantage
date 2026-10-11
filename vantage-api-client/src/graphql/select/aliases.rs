//! Column names versus server field names.
//!
//! A column can read a server field of another name (`graphql: { field: … }`
//! in YAML). The rest of the crate speaks column names only; the select
//! translates on the way out: the selection set asks for `name: field` (a
//! GraphQL alias, so the response arrives keyed by the column name) and
//! condition and sort keys are renamed to the server field.

use indexmap::IndexMap;
use vantage_expressions::Order;

use crate::graphql::condition::{FieldCondition, GraphqlCondition};
use crate::graphql::select::GraphqlSelect;

impl GraphqlSelect {
    /// The server field a column reads: its alias target, else the column
    /// name itself.
    fn wire_name<'a>(&'a self, column: &'a str) -> &'a str {
        self.aliases.get(column).map_or(column, String::as_str)
    }

    /// The selected paths as they go into the selection set, with every
    /// aliased column spelled `name: field`.
    pub(crate) fn selection_paths(&self) -> Vec<String> {
        self.fields
            .iter()
            .map(|path| match self.aliases.get(path) {
                Some(field) => format!("{path}: {field}"),
                None => path.clone(),
            })
            .collect()
    }

    /// The conditions with their field keys renamed to server fields.
    /// A [`GraphqlCondition::Deferred`] object is opaque and goes out as is.
    pub(crate) fn wire_conditions(&self) -> Vec<GraphqlCondition> {
        self.conditions
            .iter()
            .map(|c| rename(c, &self.aliases))
            .collect()
    }

    /// The sort keys renamed to server fields.
    pub(crate) fn wire_sort(&self) -> Vec<(&str, Order)> {
        self.sort
            .iter()
            .map(|(field, order)| (self.wire_name(field), *order))
            .collect()
    }
}

fn rename(condition: &GraphqlCondition, aliases: &IndexMap<String, String>) -> GraphqlCondition {
    if aliases.is_empty() {
        return condition.clone();
    }
    let wire = |field: &String| aliases.get(field).unwrap_or(field).clone();
    match condition {
        GraphqlCondition::Field(fc) => GraphqlCondition::Field(FieldCondition::new(
            wire(&fc.field),
            fc.op.clone(),
            fc.value.clone(),
        )),
        GraphqlCondition::DeferredField {
            field,
            op,
            value_fn,
        } => GraphqlCondition::deferred_field(wire(field), op.clone(), value_fn.clone()),
        GraphqlCondition::And(parts) => {
            GraphqlCondition::And(parts.iter().map(|p| rename(p, aliases)).collect())
        }
        GraphqlCondition::Or(parts) => {
            GraphqlCondition::Or(parts.iter().map(|p| rename(p, aliases)).collect())
        }
        GraphqlCondition::Not(inner) => GraphqlCondition::Not(Box::new(rename(inner, aliases))),
        GraphqlCondition::Deferred(_) => condition.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use vantage_expressions::Order;

    use crate::graphql::condition::{FilterDialect, GraphqlCondition};
    use crate::graphql::select::GraphqlSelect;

    #[tokio::test]
    async fn an_alias_renames_selection_filter_and_order() {
        let q = GraphqlSelect::new()
            .with_root_field("launches")
            .with_dialect(FilterDialect::Hasura)
            .with_field("id")
            .with_field("name")
            .with_alias("name", "mission_name")
            .with_condition(GraphqlCondition::Or(vec![
                GraphqlCondition::eq("name", json!("x")),
                GraphqlCondition::eq("id", json!("1")),
            ]))
            .with_order("name", Order::Asc)
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { launches(where: {_or: [{mission_name: {_eq: \"x\"}}, {id: {_eq: \"1\"}}]}, \
             order_by: [{mission_name: asc}]) { id name: mission_name } }"
        );
    }

    #[tokio::test]
    async fn the_aggregate_count_filters_on_the_server_field() {
        let q = GraphqlSelect::new()
            .with_root_field("launches")
            .with_dialect(FilterDialect::Hasura)
            .with_field("name")
            .with_alias("name", "mission_name")
            .with_condition(GraphqlCondition::eq("name", json!("x")))
            .render_aggregate_count("launches_aggregate")
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { launches_aggregate(where: {mission_name: {_eq: \"x\"}}) { aggregate { count } } }"
        );
    }

    #[tokio::test]
    async fn an_invalid_alias_target_is_refused() {
        let err = GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("name")
            .with_alias("name", "mission_name { secret }")
            .render()
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("GraphQL name is not valid"), "{err}");
    }
}
