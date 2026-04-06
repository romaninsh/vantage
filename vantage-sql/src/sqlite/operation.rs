use vantage_table::column::core::Column;
use vantage_table::operation::Operation;

use super::types::AnySqliteType;

impl Operation<AnySqliteType> for Column<AnySqliteType> {}
