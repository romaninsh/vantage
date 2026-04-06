pub mod expr_data_source;
pub mod selectable_data_source;
pub mod table_source;

use vantage_expressions::traits::datasource::DataSource;

use super::PostgresDB;

impl DataSource for PostgresDB {}
