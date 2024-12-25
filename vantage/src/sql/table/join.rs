use std::ops::{Deref, DerefMut};

use crate::{
    prelude::{EmptyEntity, JoinQuery, RelatedTable, Table},
    traits::datasource::DataSource,
};

// #[derive(Clone, Debug)]
// enum JoinType {
//     Inner,
//     Left,
//     Right,
//     Full,
// }

pub struct Join<T: DataSource> {
    // table: Table<T, E>,
    table: Table<T, EmptyEntity>,
    join_query: JoinQuery,
}

// impl<T: DataSource> Join<T> {
//     fn clone(&self) -> Self {
//         Join {
//             table: self.table.clone(),
//             join_query: self.join_query.clone(),
//         }
//     }
// }

impl<T: DataSource> std::fmt::Debug for Join<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Join")
            .field("table", &self.table.get_table_name())
            .field("fields", &self.table.get_columns())
            .field("join_query", &self.join_query)
            .finish()
    }
}

impl<T: DataSource> Join<T> {
    pub fn new(table: Table<T, EmptyEntity>, join_query: JoinQuery) -> Self {
        // Related table should have alias

        Join { table, join_query }
    }
    pub fn join_query(&self) -> &JoinQuery {
        &self.join_query
    }
    pub fn table(&self) -> &Table<T, EmptyEntity> {
        &self.table
    }
    pub fn table_mut(&mut self) -> &mut Table<T, EmptyEntity> {
        &mut self.table
    }
}

impl<T: DataSource> Deref for Join<T> {
    type Target = Table<T, EmptyEntity>;

    fn deref(&self) -> &Self::Target {
        &self.table
    }
}

impl<T: DataSource> DerefMut for Join<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.table
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_join() {
//         let data =
//             json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
//         let db = MockDataSource::new(&data);

//         let vip_client = Table::new("client", db)
//             .add_title_field("name")
//             .add_field("is_vip")
//             .add_field("total_spent")
//             .add_condition_on_field("is_vip", "is", "true".to_owned())
//             .unwrap();

//         let vip_details = vip_client.add_join_table("id", "vip_details", "client_id");
//         vip_details.add_field("discount");

//         let select = vip_client.get_select_query();
//         assert_eq!(
//             select.render_chunk().sql().clone(),
//             "SELECT name, total_spent, vip_details.discount FROM client JOIN vip_details ON vip_detils.client_id = client.id WHERE is_vip is {}".to_owned()
//         );
//     }
// }
