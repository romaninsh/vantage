use std::sync::Arc;

use anyhow::Result;
use serde_json::json;

use crate::{
    prelude::{PgValueColumn, SqlTable},
    sql::{query::SqlQuery, Chunk, Operations, Query},
};

use super::TableExtension;

#[derive(Debug)]
pub struct SoftDelete {
    soft_delete_field: String,
}

impl SoftDelete {
    pub fn new(soft_delete_field: &str) -> Self {
        SoftDelete {
            soft_delete_field: soft_delete_field.to_string(),
        }
    }
    fn is_deleted(&self, table: &dyn SqlTable) -> Arc<PgValueColumn> {
        table.get_column(&self.soft_delete_field).unwrap()
    }
}

impl TableExtension for SoftDelete {
    fn init(&self, table: &mut dyn SqlTable) {
        table.add_column(
            self.soft_delete_field.clone(),
            PgValueColumn::new(&self.soft_delete_field),
        );
    }

    /// When selecting records, exclude deleted records
    fn before_select_query(&self, table: &dyn SqlTable, query: &mut Query) -> Result<()> {
        query
            .get_where_conditions_mut()
            .add_condition(self.is_deleted(table).eq(&false).render_chunk());
        Ok(())
    }
    /// When deleting records, mark them as deleted instead
    fn before_delete_query(&self, _table: &dyn SqlTable, query: &mut Query) -> Result<()> {
        query.set_type(crate::sql::query::QueryType::Update);
        query.set_field_value(&self.soft_delete_field, json!(true));
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::{
        mocks::datasource::MockDataSource,
        prelude::{AnyTable, Chunk, Operations, TableWithQueries},
        sql::{table::extensions::Hooks, Table},
    };

    #[tokio::test]
    async fn test_soft_delete() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let data_source = MockDataSource::new(&data);

        let mut table = Table::new("users", data_source.clone())
            .with_column("name")
            .with_column("surname");

        table.add_condition(table.get_column("name").unwrap().eq(&"John".to_string()));

        let mut hooks = Hooks::new();
        let ext = SoftDelete::new("is_deleted");
        ext.init(&mut table);
        hooks.add_hook(Box::new(ext));

        let mut query = table.get_select_query();
        hooks.before_select_query(&mut table, &mut query).unwrap();

        let result = query.render_chunk().split();

        assert_eq!(
            result.0,
            "SELECT name, surname, is_deleted FROM users WHERE (name = {}) AND (is_deleted = {})"
        );
        assert_eq!(result.1[0], json!("John"));
        assert_eq!(result.1[1], json!(false));
    }

    #[test]
    fn test_soft_delete_integration() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let data_source = MockDataSource::new(&data);

        let table = Table::new("users", data_source.clone())
            .with_column("name")
            .with_column("surname")
            .with_extension(SoftDelete::new("is_deleted"));

        let query = table.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT name, surname, is_deleted FROM users WHERE (is_deleted = {})"
        );
        assert_eq!(query.1[0], json!(false));
    }
}
