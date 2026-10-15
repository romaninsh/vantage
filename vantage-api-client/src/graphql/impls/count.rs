//! Server-side row count through a Hasura aggregate field.
//!
//! Used by `get_table_count` when [`GraphqlApi::can_count_server_side`]
//! holds; otherwise the count falls back to listing and counting rows.

use serde_json::Value;
use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_types::Entity;

use crate::graphql::api::{GraphqlApi, GraphqlReply};
use crate::graphql::impls::table_source::select_from_table;
use crate::graphql::types::AnyGraphqlType;

/// Count the rows `table` would list — its conditions and quicksearch
/// included — with one `<count_field> { aggregate { count } }` query.
pub(crate) async fn aggregate_count<E: Entity<AnyGraphqlType>>(
    table: &Table<GraphqlApi, E>,
) -> Result<i64> {
    let api = table.data_source();
    let field = api.count_field(table.table_name());
    let rendered = select_from_table(table)
        .render_aggregate_count(&field)
        .await?;
    match api
        .post_graphql_reply(&rendered.query, &rendered.variables)
        .await?
    {
        GraphqlReply::Data(data) => parse_count(&data, &field),
        // Most often a role without `allow_aggregations`, or a schema with
        // no such field. Listing would answer, but quietly switching
        // strategies would hide a misconfiguration, so say how to fix it.
        GraphqlReply::Errors(errors) => Err(error!(
            "GraphQL server rejected the aggregate count; set `count: list` to count by listing rows",
            field = field,
            errors = errors
        )),
    }
}

/// Read `data.<field>.aggregate.count`.
fn parse_count(data: &Value, field: &str) -> Result<i64> {
    data.get(field)
        .and_then(|v| v.get("aggregate"))
        .and_then(|v| v.get("count"))
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            error!(
                "GraphQL aggregate response missing aggregate.count",
                field = field,
                got = data.to_string()
            )
        })
}
