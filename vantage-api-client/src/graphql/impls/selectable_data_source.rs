//! `SelectableDataSource<AnyGraphqlType, GraphqlCondition>` for `GraphqlApi`.
//!
//! Wires `GraphqlSelect` into the Vantage query pipeline so `table.select()`
//! returns a usable builder. `execute_select` renders the query, posts it,
//! and reshapes the response into a flat `Vec<AnyGraphqlType>` of row values.

use serde_json::Value;
use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_expressions::traits::datasource::SelectableDataSource;

use crate::graphql::api::GraphqlApi;
use crate::graphql::condition::GraphqlCondition;
use crate::graphql::select::GraphqlSelect;
use crate::graphql::types::AnyGraphqlType;

impl SelectableDataSource<AnyGraphqlType, GraphqlCondition> for GraphqlApi {
    type Select = GraphqlSelect;

    fn select(&self) -> Self::Select {
        GraphqlSelect::new()
    }

    fn add_select_column(
        &self,
        select: &mut Self::Select,
        _expression: Expression<AnyGraphqlType>,
        alias: Option<&str>,
    ) {
        // GraphQL selection sets are field names. If an alias is given,
        // we record it as the field; otherwise drop the expression
        // (mirrors Mongo's projection-only posture).
        if let Some(alias) = alias {
            select.fields.push(alias.to_string());
        }
    }

    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<AnyGraphqlType>> {
        let rendered = select.render().await?;
        let data = self
            .post_graphql(&rendered.query, &rendered.variables)
            .await?;

        // GraphQL responses nest the rows under the root field name:
        //   { "data": { "launches": [ {...}, {...} ] } }
        // `post_graphql` already peeled off the outer `data` wrapper.
        let root = select
            .root_field
            .as_deref()
            .ok_or_else(|| error!("GraphqlSelect has no root_field set"))?;

        let rows = data.get(root).ok_or_else(|| {
            error!(
                "GraphQL response missing expected root field",
                field = root.to_string()
            )
        })?;

        match rows {
            Value::Array(arr) => Ok(arr
                .iter()
                .map(|v| AnyGraphqlType::untyped(v.clone()))
                .collect()),
            Value::Object(_) => Ok(vec![AnyGraphqlType::untyped(rows.clone())]),
            Value::Null => Ok(Vec::new()),
            other => Err(error!(
                "Unexpected GraphQL response shape under root field",
                field = root.to_string(),
                got = format!("{:?}", other)
            )),
        }
    }
}
