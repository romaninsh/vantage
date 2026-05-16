//! Compile-time verification that REST and GraphQL tables both bridge
//! into `AnyTable` via the blanket `CborAdapter` in `vantage-table`.
//!
//! No network — the goal is to prove the type bounds line up
//! (`Value: Into<CborValue> + From<CborValue>`, `Id: Display + From<String>`).

use vantage_api_client::{GraphqlApi, RestApi};
use vantage_table::any::AnyTable;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

#[test]
fn rest_table_wraps_into_any_table() {
    let api = RestApi::new("https://example.test");
    let table: Table<RestApi, EmptyEntity> = Table::new("users", api);
    let any = AnyTable::from_table(table);
    assert!(any.datasource_name().contains("RestApi"));
}

#[test]
fn graphql_table_wraps_into_any_table() {
    let api = GraphqlApi::new("https://example.test/graphql");
    let table: Table<GraphqlApi, EmptyEntity> = Table::new("launches", api);
    let any = AnyTable::from_table(table);
    assert!(any.datasource_name().contains("GraphqlApi"));
}

#[test]
fn rest_and_graphql_tables_live_in_same_collection() {
    // A heterogeneous Vec<AnyTable> — the point of multi-backend.
    let rest = AnyTable::from_table(Table::<RestApi, EmptyEntity>::new(
        "users",
        RestApi::new("https://rest.test"),
    ));
    let graphql = AnyTable::from_table(Table::<GraphqlApi, EmptyEntity>::new(
        "launches",
        GraphqlApi::new("https://graphql.test/graphql"),
    ));

    let tables: Vec<AnyTable> = vec![rest, graphql];
    assert_eq!(tables.len(), 2);
    assert!(tables[0].datasource_name().contains("RestApi"));
    assert!(tables[1].datasource_name().contains("GraphqlApi"));
}
