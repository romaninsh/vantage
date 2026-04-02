pub mod base;
pub mod expr_data_source;
// pub mod table_source;

use vantage_expressions::traits::datasource::DataSource;

use crate::surrealdb::SurrealDB;

impl DataSource for SurrealDB {}
