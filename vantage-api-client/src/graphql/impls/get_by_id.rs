//! Single-row fetch by id for `GraphqlApi`.
//!
//! What reaches the wire follows [`Supports`](crate::graphql::api::Supports):
//! the id becomes a filter only with filter push-down on, and `limit: 1`
//! is sent only with paging on. A root field taking no filter is listed in
//! full and the row picked out locally.

use serde_json::Value;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{Entity, Record};

use crate::graphql::api::GraphqlApi;
use crate::graphql::impls::local_filter::pluck;
use crate::graphql::impls::table_source::{
    list_rows, row_to_record, rows_under_root, select_from_table, value_to_string,
};
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::types::AnyGraphqlType;

/// Integer column types, by the name `Column::get_type` reports.
const INTEGER_TYPES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
];

/// The id as the JSON value the server compares it with: an integer for an
/// integer-typed id column, a string otherwise.
fn id_value<E: Entity<AnyGraphqlType>>(table: &Table<GraphqlApi, E>, id: &str) -> Result<Value> {
    let column = table.id_field();
    id_for_column(column, id, &table.id_field_name())
}

/// `id` as the JSON value `column` compares with: an integer when the
/// column is integer-typed, a string otherwise (or without a column).
/// `name` labels the error.
pub(crate) fn id_for_column(
    column: Option<&Column<AnyGraphqlType>>,
    id: &str,
    name: &str,
) -> Result<Value> {
    if !column.is_some_and(|c| INTEGER_TYPES.contains(&c.get_type())) {
        return Ok(Value::String(id.to_string()));
    }
    id.parse::<i64>().map(Value::from).map_err(|_| {
        error!(
            "Id is not an integer for an integer id column",
            id = id,
            column = name
        )
    })
}

pub(crate) async fn get_by_id<E: Entity<AnyGraphqlType>>(
    api: &GraphqlApi,
    table: &Table<GraphqlApi, E>,
    id: &str,
) -> Result<Option<Record<AnyGraphqlType>>> {
    let id_value = id_value(table, id)?;
    let id_name = table.id_field_name();

    // Without a filter argument a `limit` would cut the list before the id
    // is found, so list everything (narrowed locally) and pick the row by
    // its id column. The raw rows are searched rather than a keyed listing,
    // so an unrelated row with a missing or repeated id can't get in the way.
    if !api.can_filter() {
        let select = select_from_table(table);
        let rows = list_rows(table, &select).await?;
        api.report_rows(rows.len());
        let found = rows
            .iter()
            .find(|row| pluck(row, &id_name).is_some_and(|v| value_to_string(v) == id));
        return match found {
            Some(row) => Ok(Some(row_to_record(row, &select.fields, Some(&id_name))?.1)),
            None => Ok(None),
        };
    }

    let mut select = select_from_table(table);
    select
        .conditions
        .push(Column::<AnyGraphqlType>::new(&id_name).eq(AnyGraphqlType::untyped(id_value)));
    select.skip = None;
    select.limit = api.can_page().then_some(1);

    let rendered = select.render().await?;
    let data = api
        .post_graphql(&rendered.query, &rendered.variables)
        .await?;
    let arr = rows_under_root(api, table.table_name(), &data, true)?;
    api.report_rows(arr.len());
    match arr.into_iter().next() {
        Some(row) => {
            let (_id, rec) = row_to_record(&row, &select.fields, Some(&id_name))?;
            Ok(Some(rec))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vantage_types::EmptyEntity;

    fn api() -> GraphqlApi {
        GraphqlApi::new("https://api.test/graphql")
    }

    #[test]
    fn an_integer_id_column_takes_an_integer() {
        let mut table = Table::<GraphqlApi, EmptyEntity>::new("launches", api());
        table.add_column_of::<i64>("id");
        table.set_id_field("id");
        assert_eq!(id_value(&table, "42").unwrap(), json!(42));
        let err = id_value(&table, "abc").unwrap_err().to_string();
        assert!(err.contains("Id is not an integer"), "{err}");
        // Parsed exactly: surrounding whitespace is not an integer either.
        assert!(id_value(&table, " 42").is_err());
    }

    #[test]
    fn a_string_id_column_takes_a_string() {
        let table = Table::<GraphqlApi, EmptyEntity>::new("launches", api()).with_id_column("id");
        assert_eq!(id_value(&table, "42").unwrap(), json!("42"));
    }
}
