use vantage_core::Result;
use vantage_table::traits::table_query_source::TableQuerySource;
use vantage_types::Entity;

use crate::surrealdb::SurrealDB;
use crate::types::AnySurrealType;

impl TableQuerySource<AnySurrealType> for SurrealDB {
    fn get_table_select_query<E: Entity<AnySurrealType>>(
        &self,
        table: &vantage_table::table::Table<Self, E>,
    ) -> Result<Self::Select> {
        Ok(table.select())
    }
}
