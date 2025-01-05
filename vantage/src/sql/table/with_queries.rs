use indexmap::IndexMap;
use serde::Serialize;
use serde_json::{to_value, Value};
use std::sync::Arc;

use super::column::SqlColumn;
use super::{AnyTable, PgValueColumn, SqlTable, TableWithColumns};
use crate::prelude::AssociatedQuery;
use crate::sql::query::{QueryType, SqlQuery};
use crate::sql::table::Table;
use crate::sql::Query;
use crate::traits::column::SqlField;
use crate::traits::datasource::DataSource;
use crate::traits::entity::Entity;

use super::RelatedTable;

pub trait TableWithQueries: AnyTable {
    fn get_empty_query(&self) -> Query;
    fn get_select_query(&self) -> Query;
    fn get_select_query_for_fields(
        &self,
        fields: IndexMap<String, Arc<Box<dyn SqlField>>>,
    ) -> Query;
    fn get_select_query_for_field_names(&self, field_names: &[&str]) -> Query;
    fn get_select_query_for_field(&self, field: Box<dyn SqlField>) -> Query;
}

impl<T: DataSource, E: Entity> TableWithQueries for Table<T, E> {
    fn get_empty_query(&self) -> Query {
        let mut query =
            Query::new().with_table(&self.table_name, self.get_alias().try_get_for_from());
        for condition in self.conditions.iter() {
            query = query.with_condition(condition.clone());
        }
        for (_alias, join) in &self.joins {
            query = query.with_join(join.join_query().clone());
        }
        query
    }

    fn get_select_query(&self) -> Query {
        let mut query = self.get_empty_query();
        query = self.add_columns_into_query(query, None);
        self.hooks.before_select_query(self, &mut query).unwrap();
        query
    }

    fn get_select_query_for_fields(
        &self,
        fields: IndexMap<String, Arc<Box<dyn SqlField>>>,
    ) -> Query {
        let mut query = self.get_empty_query();
        for (field_alias, field_val) in fields {
            let field_val = field_val.clone();
            query.add_field(Some(field_alias), field_val);
        }
        query
    }

    fn get_select_query_for_field_names(&self, field_names: &[&str]) -> Query {
        let mut index_map = IndexMap::new();
        for field_name in field_names {
            let field = self.search_for_field(field_name).unwrap();
            index_map.insert(field_name.to_string(), Arc::new(field));
        }
        self.get_select_query_for_fields(index_map)
    }

    fn get_select_query_for_field(&self, field: Box<dyn SqlField>) -> Query {
        let mut q = self.get_empty_query();
        q.add_field(None, Arc::new(field));
        self.hooks.before_select_query(self, &mut q).unwrap();
        q
    }
}

impl<D: DataSource, E: Entity> Table<D, E> {
    pub fn field_query(&self, field: Arc<PgValueColumn>) -> AssociatedQuery<D, E> {
        // let query = self.get_select_query_for_field(field);
        let query = self.get_empty_query().with_field(field.name(), field);
        AssociatedQuery::new(query, self.data_source.clone())
    }

    pub fn query(&self) -> AssociatedQuery<D, E> {
        AssociatedQuery::new(
            self.get_select_query_for_struct(E::default()),
            self.data_source.clone(),
        )
    }

    pub fn query_for_fields(
        &self,
        fields: IndexMap<String, Arc<Box<dyn SqlField>>>,
    ) -> AssociatedQuery<D, E> {
        AssociatedQuery::new(
            self.get_select_query_for_fields(fields),
            self.data_source.clone(),
        )
    }

    pub fn query_for_field_names(&self, field_names: &[&str]) -> AssociatedQuery<D, E> {
        AssociatedQuery::new(
            self.get_select_query_for_field_names(field_names),
            self.data_source.clone(),
        )
    }

    pub fn get_select_query_for_struct<R: Serialize>(&self, default: R) -> Query {
        let json_value = to_value(default).unwrap();

        let field_names = match json_value {
            Value::Object(map) => {
                let field_names = map.keys().cloned().collect::<Vec<String>>();
                field_names
            }
            _ => panic!("Expected argument to be a struct"),
        };

        let i = field_names
            .into_iter()
            .filter_map(|f| self.search_for_field(&f).map(|c| (f, Arc::new(c))));

        let i = i.collect::<IndexMap<_, _>>();

        let mut q = self.get_select_query_for_fields(i);
        self.hooks.before_select_query(self, &mut q).unwrap();
        q
    }

    pub fn get_insert_query<E2>(&self, values: E2) -> Query
    where
        E2: Serialize,
    {
        let mut query = Query::new()
            .with_table(&self.table_name, None)
            .with_type(QueryType::Insert);

        let serde_json::Value::Object(value_map) = serde_json::to_value(values).unwrap() else {
            panic!("Values must be a struct");
        };

        for (field, _) in &self.columns {
            let field_object = Arc::new(PgValueColumn::new(&field));

            if field_object.calculated() {
                continue;
            };

            let Some(value) = value_map.get(field) else {
                continue;
            };

            query = query.with_set_field(field, value.clone());
        }
        query
    }

    pub fn get_update_query<E2>(&self, values: E2) -> Query
    where
        E2: Serialize,
    {
        let mut query = Query::new()
            .with_table(&self.table_name, None)
            .with_type(QueryType::Update);

        let serde_json::Value::Object(value_map) = serde_json::to_value(values).unwrap() else {
            panic!("Values must be a struct");
        };

        for (field, _) in &self.columns {
            let field_object = Arc::new(PgValueColumn::new(&field.clone()));

            if field_object.calculated() {
                continue;
            };

            let Some(value) = value_map.get(field) else {
                continue;
            };

            query = query.with_set_field(field, value.clone());
        }
        for condition in self.conditions.iter() {
            query = query.with_condition(condition.clone());
        }
        query
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::prelude::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use crate::{expr_arc, mocks::datasource::MockDataSource, prelude::Chunk};

    #[derive(Serialize, Deserialize, Clone, Default)]
    struct User {
        name: String,
        surname: String,
    }

    impl Entity for User {}

    #[test]
    fn test_insert_query() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let db = MockDataSource::new(&data);

        let table: Table<MockDataSource, User> = Table::new_with_entity("users", db)
            .with_column("name")
            .with_column("surname");

        let query = table
            .get_insert_query(User {
                name: "John".to_string(),
                surname: "Doe".to_string(),
            })
            .render_chunk()
            .split();

        assert_eq!(
            query.0,
            "INSERT INTO users (name, surname) VALUES ({}, {}) returning id"
        );
        assert_eq!(query.1[0], json!("John"));
        assert_eq!(query.1[1], json!("Doe"));
    }

    #[test]
    fn test_update_query() {
        #[derive(Serialize, Deserialize, Clone)]
        struct UserName {
            name: String,
        }

        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let db = MockDataSource::new(&data);

        let table = Table::new("users", db)
            .with_id_column("id")
            .with_id(1.into())
            .with_column("name")
            .with_column("surname");

        let query = table
            .get_update_query(UserName {
                name: "John".to_string(),
            })
            .render_chunk()
            .split();

        assert_eq!(query.0, "UPDATE users SET name = {} WHERE (id = {})");
        assert_eq!(query.1[0], json!("John"));
        assert_eq!(query.1[1], json!(1));
    }

    #[test]
    fn test_expression_query() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let mut orders = Table::new("orders", db.clone())
            .with_column("price")
            .with_column("qty");

        orders.add_expression("total", |t| {
            expr_arc!(
                "{}*{}",
                t.get_column("price").unwrap().clone(),
                t.get_column("qty").unwrap().clone()
            )
            .render_chunk()
        });

        let query = orders.get_select_query().render_chunk().split();

        assert_eq!(query.0, "SELECT price, qty FROM orders");

        #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
        struct ItemLine {
            price: f64,
            qty: i32,
            total: f64,
        }

        let query = orders
            .get_select_query_for_struct(ItemLine::default())
            .render_chunk()
            .split();
        assert_eq!(
            query.0,
            "SELECT price, qty, (price*qty) AS total FROM orders"
        );
    }
}
