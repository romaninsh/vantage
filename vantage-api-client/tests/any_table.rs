//! Compile-time verification that GraphQL tables bridge into
//! `AnyTable` via the blanket `CborAdapter` in `vantage-table`.
//!
//! No network — the goal is to prove the type bounds line up
//! (`Value: Into<CborValue> + From<CborValue>`, `Id: Display + From<String>`).
//!
//! REST's half of this test is gone — the REST Vista shell no longer
//! routes references through `AnyTable`. The GraphQL half stays until
//! the matching shell rewrite ships.

use vantage_api_client::GraphqlApi;
use vantage_table::any::AnyTable;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

#[test]
fn graphql_table_wraps_into_any_table() {
    let api = GraphqlApi::new("https://example.test/graphql");
    let table: Table<GraphqlApi, EmptyEntity> = Table::new("launches", api);
    let any = AnyTable::from_table(table);
    assert!(any.datasource_name().contains("GraphqlApi"));
}
