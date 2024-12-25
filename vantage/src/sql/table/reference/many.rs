use std::sync::Arc;

use super::{RelatedSqlTable, RelatedTableFx};
use crate::{
    prelude::SqlTable,
    sql::{Chunk, Operations},
};

#[derive(Clone)]
pub struct ReferenceMany {
    target_foreign_key: String,
    get_table: Arc<Box<RelatedTableFx>>,
}

impl ReferenceMany {
    pub fn new(
        foreign_key: &str,
        get_table: impl Fn() -> Box<dyn SqlTable> + Send + Sync + 'static,
    ) -> ReferenceMany {
        ReferenceMany {
            target_foreign_key: foreign_key.to_string(),
            get_table: Arc::new(Box::new(get_table)),
        }
    }
}

impl std::fmt::Debug for ReferenceMany {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceMany")
            .field("foreign_key", &self.target_foreign_key)
            .finish()
    }
}

impl RelatedSqlTable for ReferenceMany {
    fn get_related_set(&self, table: &dyn SqlTable) -> Box<dyn SqlTable> {
        let mut target = (self.get_table)();
        let target_field = target.get_column(&self.target_foreign_key).unwrap();
        let id_set = table.get_select_query_for_field(Box::new(table.id()));
        target.add_condition(target_field.in_expr(&id_set));
        target
    }

    fn get_linked_set(&self, table: &dyn SqlTable) -> Box<dyn SqlTable> {
        let mut target = (self.get_table)();
        let target_field = target
            .get_column(&self.target_foreign_key)
            .unwrap()
            .with_table_alias();
        dbg!(&table.id().with_table_alias().render_chunk().preview());
        target.add_condition(target_field.eq(&table.id().with_table_alias()));
        target
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mocks::datasource::MockDataSource;
    use crate::prelude::TableWithColumns;
    use crate::sql::Table;
    use crate::traits::entity::EmptyEntity;

    #[test]
    fn test_related_reference() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let data_source = MockDataSource::new(&data);

        let users = Table::new("users", data_source.clone())
            .with_id_column("id")
            .with_title_column("name");

        let orders = Table::new("orders", data_source.clone())
            .with_id_column("id")
            .with_column("user_id")
            .with_title_column("order_ref");

        let reference = ReferenceMany::new("user_id", move || Box::new(orders.clone()));

        let target = reference.get_related_set(&users);

        assert_eq!(
            target.get_select_query().preview(),
            "SELECT id, user_id, order_ref FROM orders WHERE (user_id IN (SELECT id FROM users))"
        );

        let target = reference.get_linked_set(&users);

        assert_eq!(
            target.get_select_query().preview(),
            "SELECT id, user_id, order_ref FROM orders WHERE (orders.user_id = users.id)"
        );

        // lets try downcasting

        let target = Box::new(target.as_any_ref());
        let target = target.downcast_ref::<Table<MockDataSource, EmptyEntity>>();
        let target = target.unwrap();

        let q = target.field_query(target.id());
        assert_eq!(
            q.preview(),
            "SELECT id FROM orders WHERE (orders.user_id = users.id)"
        )
    }
}
