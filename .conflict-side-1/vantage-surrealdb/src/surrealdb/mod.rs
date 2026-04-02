pub mod impls;

use std::sync::Arc;

use surreal_client::SurrealClient;

// pub mod selectsource;
// pub mod tablesource;

// Create a wrapper for shared SurrealDB state
#[derive(Clone)]
pub struct SurrealDB {
    inner: Arc<tokio::sync::Mutex<SurrealClient>>,
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::{
//         operation::{Expressive, RefOperation},
//         select::SurrealSelect,
//         thing::Thing,
//     };
//     use surreal_client::Engine;
//     use vantage_expressions::{
//         expr,
//         protocol::{expressive::IntoExpressive, selectable::Selectable},
//     };

//     #[tokio::test]
//     async fn test_select_with_thing_reference() {
//         let shared_db = setup_test_db().await;
//         let mut select = SurrealSelect::new();
//         select.set_source("product", None);
//         select.add_where_condition(expr!("bakery = {}", (Thing::new("bakery", "hill_valley"))));
//         select.add_where_condition(expr!("is_deleted = {}", false));
//         select.add_order_by(expr!("name"), true);

//         let result = shared_db.execute(&select.expr()).await;
//         println!("✅ Select with Thing reference: {:?}", result);
//     }

//     #[tokio::test]
//     async fn test_select_with_specific_fields() {
//         let shared_db = setup_test_db().await;
//         let mut select = SurrealSelect::new();
//         select.add_field("name");
//         select.add_field("price");
//         select.set_source("product", None);
//         select.add_where_condition(expr!("price > {}", 100));

//         let result = shared_db.execute(&select.expr()).await;
//         println!("✅ Select with specific fields: {:?}", result);
//     }

//     #[tokio::test]
//     async fn test_select_with_relationship_traversal() {
//         let shared_db = setup_test_db().await;
//         let mut select = SurrealSelect::new();
//         select.set_source(
//             Thing::new("bakery", "hill_valley").rref("owns", "product"),
//             None,
//         );
//         select.add_where_condition(expr!("is_deleted = {}", false));
//         select.add_order_by(expr!("name"), true);

//         let result = shared_db.execute(&select.expr()).await;
//         println!("✅ Select with relationship traversal: {:?}", result);
//     }

//     #[tokio::test]
//     async fn test_select_with_left_relationship() {
//         let shared_db = setup_test_db().await;
//         let mut select = SurrealSelect::new();
//         select.set_source(
//             Thing::new("bakery", "hill_valley").lref("belongs_to", "client"),
//             None,
//         );
//         select.add_order_by(expr!("name"), true);

//         let result = shared_db.execute(&select.expr()).await;
//         println!("✅ Select with left relationship: {:?}", result);
//     }

//     #[tokio::test]
//     async fn test_complex_nested_query() {
//         let shared_db = setup_test_db().await;
//         // Build a more complex query similar to the ones in select.rs tests
//         let subquery = SurrealSelect::new()
//             .with_source("order")
//             .with_condition(expr!("status = {}", "completed"))
//             .expr();

//         let mut main_select = SurrealSelect::new();
//         main_select.add_field("name");
//         main_select.add_field("email");
//         main_select.set_source("client", None);
//         main_select.add_where_condition(expr!("id IN ({})", (subquery)));

//         let result = shared_db.execute(&main_select.expr()).await;
//         println!("✅ Complex nested query: {:?}", result);
//     }

//     #[test]
//     fn test_prepare_query_conversion() {
//         // Create mock client for testing

//         let db = SurrealDB::new(SurrealClient::new(Box::new(MockEngine), None, None));

//         let expr = expr!(
//             "SELECT * FROM product WHERE price > {} AND name = {}",
//             100,
//             "bread"
//         );
//         let (query, params) = db.prepare_query(&expr);

//         assert_eq!(
//             query,
//             "SELECT * FROM product WHERE price > $_arg1 AND name = $_arg2"
//         );
//         assert_eq!(params.len(), 2);
//         assert_eq!(params.get("_arg1"), Some(&Value::Number(100.into())));
//         assert_eq!(
//             params.get("_arg2"),
//             Some(&Value::String("bread".to_string()))
//         );
//     }

//     #[test]
//     fn test_prepare_query_with_nested_expression() {
//         // Create mock client for testing

//         let db = SurrealDB::new(SurrealClient::new(Box::new(MockEngine), None, None));

//         let nested = expr!("SELECT id FROM client WHERE active = {}", true);
//         let main_expr = expr!("SELECT * FROM product WHERE owner IN ({})", (nested));

//         let (query, params) = db.prepare_query(&main_expr);

//         assert!(query.contains("$_arg"));
//         assert!(!params.is_empty());
//         println!("Query: {}", query);
//         println!("Params: {:?}", params);
//     }

//     #[tokio::test]
//     async fn test_expression_integration() {
//         let shared_db = setup_test_db().await;
//         // Test direct expression execution
//         let query = expr!("SELECT name, price FROM product WHERE price > {}", 200);
//         let result = shared_db.execute(&query).await;
//         println!("✅ Direct expression execution: {:?}", result);

//         // Test with multiple parameters
//         let multi_param_query = expr!(
//             "SELECT * FROM product WHERE price BETWEEN {} AND {} AND category = {}",
//             50,
//             200,
//             "pastry"
//         );
//         let result2 = shared_db.execute(&multi_param_query).await;
//         println!("✅ Multi-parameter query: {:?}", result2);
//     }
// }
