pub mod base;
pub mod expr_data_source;
pub mod selectable_data_source;
pub mod table_expr_source;
pub mod table_query_source;
pub mod table_source;

use vantage_expressions::traits::datasource::DataSource;

use crate::surrealdb::SurrealDB;

impl DataSource for SurrealDB {}
