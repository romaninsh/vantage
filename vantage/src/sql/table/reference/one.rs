use std::sync::Arc;

use super::{RelatedSqlTable, RelatedTableFx};
use crate::{prelude::SqlTable, sql::Operations};

#[derive(Clone)]
pub struct ReferenceOne {
    our_foreign_key: String,
    get_table: Arc<Box<RelatedTableFx>>,
}

impl ReferenceOne {
    pub fn new(
        our_foreign_key: &str,
        get_table: impl Fn() -> Box<dyn SqlTable> + Send + Sync + 'static,
    ) -> ReferenceOne {
        ReferenceOne {
            our_foreign_key: our_foreign_key.to_string(),
            get_table: Arc::new(Box::new(get_table)),
        }
    }
}

impl std::fmt::Debug for ReferenceOne {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReferenceOne")
            .field("foreign_key", &self.our_foreign_key)
            .finish()
    }
}

impl RelatedSqlTable for ReferenceOne {
    fn get_related_set(&self, table: &dyn SqlTable) -> Box<dyn SqlTable> {
        let mut target = (self.get_table)();
        let target_field = target.id();
        let id_set = table.get_select_query_for_field(Box::new(
            table.get_column(self.our_foreign_key.as_str()).unwrap(),
        ));
        target.add_condition(target_field.in_expr(&id_set));
        target
    }

    fn get_linked_set(&self, table: &dyn SqlTable) -> Box<dyn SqlTable> {
        let mut target = (self.get_table)();
        let target_field = target.id().with_table_alias();
        target.add_condition(
            target_field.eq(&table
                .get_column(self.our_foreign_key.as_str())
                .unwrap()
                .with_table_alias()),
        );
        target
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::mocks::datasource::MockDataSource;
    use crate::sql::Table;

    #[test]
    fn test_related_one_reference() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let data_source = MockDataSource::new(&data);

        let users = Table::new("users", data_source.clone())
            .with_id_column("id")
            .with_title_column("name")
            .with_column("role_id");

        let roles = Table::new("roles", data_source.clone())
            .with_id_column("id")
            .with_title_column("name");

        let reference = ReferenceOne::new("role_id", move || Box::new(roles.clone()));

        let target = reference.get_related_set(&users);

        assert_eq!(
            target.get_select_query().preview(),
            "SELECT id, name FROM roles WHERE (id IN (SELECT role_id FROM users))"
        );

        let target = reference.get_linked_set(&users);

        assert_eq!(
            target.get_select_query().preview(),
            "SELECT id, name FROM roles WHERE (roles.id = users.role_id)"
        );
    }
}
