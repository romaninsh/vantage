use std::sync::Arc;

use anyhow::{Context, Result, anyhow};

use super::reference::{RelatedSqlTable, many::ReferenceMany, one::ReferenceOne};
use crate::sql::Chunk;
use crate::traits::datasource::DataSource;
use crate::traits::entity::Entity;
use crate::{prelude::EmptyEntity, sql::table::Table};

use super::SqlTable;

impl<T: DataSource, E: Entity> Table<T, E> {
    pub fn with_many(
        mut self,
        relation: &str,
        foreign_key: &str,
        cb: impl Fn() -> Box<dyn SqlTable> + Send + Sync + 'static,
    ) -> Self {
        self.add_ref(relation, Box::new(ReferenceMany::new(foreign_key, cb)));
        self
    }

    pub fn with_one(
        mut self,
        relation: &str,
        foreign_key: &str,
        cb: impl Fn() -> Box<dyn SqlTable> + Send + Sync + 'static,
    ) -> Self {
        // let client = cb();
        self.add_ref(relation, Box::new(ReferenceOne::new(foreign_key, cb)));

        self
    }

    pub fn add_imported_fields(&mut self, relation: &str, field_names: &[&str]) {
        for field_name in field_names {
            let field_name = field_name.to_string();
            let name = format!("{}_{}", &relation, &field_name);
            let relation = relation.to_string();
            self.add_expression(&name, move |t| {
                let tt = t
                    .get_subquery(&relation)
                    .with_context(|| format!("Failed to get subquery for '{}'", &relation))
                    .unwrap();

                tt.get_select_query_for_field(Box::new(tt.get_column(&field_name).unwrap()))
                    .render_chunk()
            });
        }
    }

    pub fn with_imported_fields(mut self, relation: &str, field_names: &[&str]) -> Self {
        self.add_imported_fields(relation, field_names);
        self
    }

    pub fn add_ref(&mut self, relation: &str, reference: Box<dyn RelatedSqlTable>) {
        self.refs.insert(relation.to_string(), Arc::new(reference));
    }

    pub fn get_ref(&self, ref_name: &str) -> Result<Box<dyn SqlTable>> {
        self.refs
            .get(ref_name)
            .map(|r| r.get_related_set(self))
            .ok_or_else(|| anyhow!("Reference not found"))
    }

    pub fn get_ref_with_empty_entity(&self, ref_name: &str) -> Result<Table<T, EmptyEntity>> {
        let t = self.get_ref(ref_name)?;
        let t = Box::new(t.as_any_ref());
        let t = t.downcast_ref::<Table<T, EmptyEntity>>().unwrap().clone();
        Ok(t)
    }

    pub fn get_subquery(&self, ref_name: &str) -> Result<Box<dyn SqlTable>> {
        let Some(r) = self.refs.get(ref_name) else {
            return Err(anyhow!("Reference not found"));
        };

        Ok(r.get_linked_set(self))
    }

    pub fn get_subquery_as<E2: Entity>(&self, ref_name: &str) -> Result<Table<T, E2>> {
        let Some(r) = self.refs.get(ref_name) else {
            return Err(anyhow!("Reference not found"));
        };

        r.get_linked_set(self)
            .as_any_ref()
            .downcast_ref::<Table<T, E2>>()
            .ok_or_else(|| anyhow!("Failed to downcast to specific table type"))
            .cloned()
    }

    pub fn get_ref_as<T2: DataSource, E2: Entity>(&self, field: &str) -> Result<Table<T2, E2>> {
        let table = self.get_ref(field)?;
        table
            // TODO: not sure why we can't as_any().downcast() here
            .as_any_ref()
            .downcast_ref::<Table<T2, E2>>()
            .ok_or_else(|| anyhow!("Failed to downcast to specific table type"))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use serde_json::json;

    use crate::{mocks::datasource::MockDataSource, prelude::*};

    #[test]
    fn test_father_child() {
        struct PersonSet {}
        impl PersonSet {
            fn table() -> Table<MockDataSource, EmptyEntity> {
                let data = json!([]);
                let db = MockDataSource::new(&data);
                let table = Table::new("persons", db)
                    .with_column("id")
                    .with_column("name")
                    .with_column("parent_id")
                    .with_one("parent", "parent_id", || Box::new(PersonSet::table()))
                    .with_many("children", "parent_id", || Box::new(PersonSet::table()));

                table
            }
        }

        let mut john = PersonSet::table();
        john.add_condition(john.get_column("name").unwrap().eq(&"John".to_string()));

        let children: Table<MockDataSource, EmptyEntity> = john.get_ref_as("children").unwrap();

        let query = children.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT id, name, parent_id FROM persons WHERE (parent_id IN (SELECT id FROM persons WHERE (name = {})))"
        );

        let grand_children = john
            .get_ref_as::<MockDataSource, EmptyEntity>("children")
            .unwrap()
            .get_ref_as::<MockDataSource, EmptyEntity>("children")
            .unwrap();

        let query = grand_children.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT id, name, parent_id FROM persons WHERE \
            (parent_id IN (SELECT id FROM persons WHERE \
            (parent_id IN (SELECT id FROM persons WHERE (name = {})\
            ))\
            ))"
        );

        let parent_name = john
            .get_ref_with_empty_entity("parent")
            .unwrap()
            .field_query(john.get_column("name").unwrap());

        let query = parent_name.render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT name FROM persons WHERE (id IN (SELECT parent_id FROM persons WHERE (name = {})))"
        );
    }

    #[test]
    fn test_field_importing() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let data_source = MockDataSource::new(&data);

        let users = Table::new("users", data_source.clone())
            .with_id_column("id")
            .with_title_column("name");

        let orders = Table::new("orders", data_source.clone())
            .with_id_column("id")
            .with_column("user_id")
            .with_column("sum")
            .with_title_column("ref");

        let mut users = users.with_many("orders", "user_id", move || Box::new(orders.clone()));
        users.add_expression("orders_sum", |t| {
            let x = t.get_subquery_as::<EmptyEntity>("orders").unwrap();
            x.sum(x.get_column("sum").unwrap()).render_chunk()
        });

        let q = users.get_select_query_for_field_names(&["name", "orders_sum"]);
        assert_eq!(
            q.preview(),
            "SELECT name, (SELECT (SUM(sum)) AS sum FROM orders WHERE (orders.user_id = users.id)) AS orders_sum FROM users"
        );
    }

    #[test]
    fn test_import_fields() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let data_source = MockDataSource::new(&data);

        let users = Table::new("users", data_source.clone())
            .with_id_column("id")
            .with_title_column("name")
            .with_column("role_id");

        let roles = Table::new("roles", data_source.clone())
            .with_id_column("id")
            .with_title_column("name")
            .with_column("permission");

        let users = users
            .with_one("role", "role_id", move || Box::new(roles.clone()))
            .with_imported_fields("role", &["name", "permission"]);

        assert_eq!(
            users
                .get_select_query_for_field_names(&["name", "role_name", "role_permission"])
                .preview(),
            "SELECT name, (SELECT name FROM roles WHERE (roles.id = users.role_id)) AS role_name, (SELECT permission FROM roles WHERE (roles.id = users.role_id)) AS role_permission FROM users"
        );
    }
}
