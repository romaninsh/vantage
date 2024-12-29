use anyhow::anyhow;
use std::ptr::eq;
use std::sync::Arc;

use super::{Join, SqlTable, TableWithColumns};
use crate::prelude::Chunk;
use crate::sql::query::{JoinQuery, JoinType, QueryConditions};
use crate::sql::table::Table;
use crate::sql::Operations;
use crate::traits::datasource::DataSource;
use crate::traits::entity::Entity;
use crate::uniqid::UniqueIdVendor;

use super::{AnyTable, RelatedTable};

/// # Joins
///
/// A [`Table`] can be joined to another table(s). A join will not affect number
/// of records contained in the original set, however it will add extra fields
/// from the joined table(s).
///
/// To understand this, lets consider that we have 10 records in the `product`
/// table. The `inventory` table only has 5 records (not all products have
/// inventory yet):
///
/// ```
/// struct ProductInventory {
///     name: String,
///     qty: Option<i32>,
/// }
///
/// let product = Table::new("product", db)
///    .with_id_field("id")
///    .with_field("name");
///
/// let product_inventory = product
///     .with_join(
///         Table::new("inventory", db)
///             .with_field("product_id")
///             .with_field("qty"),
///         "product_id"
///     );
///
/// product_inventory.insert(
///     ProductInventory {
///         name: "foo".to_string(),
///         qty: Some(10),
///     }
/// ).await?;
/// ```
///
/// By joining the `product` with the `inventory` table, number of records you can
/// load remains the same - 10, however only 5 of those would have `qty` field.
///
/// Once join is established, inserting a new record will additionally create a new
/// record in the `inventory` table. So at the end of the example, you should have
/// 11 records in the `product` table and 6 records in the `inventory` table.
///
/// When table is building query, it will use a `LEFT JOIN` by default. The table
/// you are joining with, can have some conditions defined, and those conditions
/// will not impact the number of records in your DataSet as they will be applied
/// under the `ON` clause.
///
/// ```
/// let mut inventory = Table::new("inventory", db)
///     .with_field("product_id")
///     .with_field("is_deleted")
///     .with_field("qty");
/// inventory.add_condition(inventory.get_field("is_deleted").unwrap().eq(false));
///
/// let mut product_inventory = product.with_join(inventory, "product_id");
/// ```
///
/// You can reference fields from the joined tables and set a condition on `product_inventory`
/// too:
///
/// ```
/// product_inventory.add_condition(product_inventory.search_for_field("qty").unwrap().gt(10));
/// ```
///
/// In this case the resulting DataSet will be affected as the new condition will be under `WHERE` clause
/// of the main query.
///
/// TODO: Actually implement this!!
///
/// ## Limitations
/// Other types of joins are likely to affect number of records in the set or will make it impossible
/// to add new records and therefore are currently not supported.
///
///  - `product.product_type_id` referencing `product_type.id` may result in multiple records re-using
///     the same type. Adding new product could result in type duplicates if not handled properly.
///  - `product.id` referencing `product_details.id` may result in some ambiguity depending on the
///     implementation.
///  - not using `LEFT JOIN` may result some original records become nullable.
///  - joining a subquery rather than a table can be handy, but will not work consistently with
///    records being modified.
///
/// One way to handle this is by using [`Query::with_join()`] with [`JoinQuery`]:
///
/// ```
/// let query = product.get_query()
///     .with_join(
///         JoinType::Inner,
///         QuerySource::Table(inventory),
///         QueryConditions::on().add_condition(
///             product.get_field("id").unwrap().eq(
///                 inventory.get_field("product_id").unwrap()
///             )
///         )
///      );
///
/// for product: ProductInventory in query.get_as().await? {
///     println!("Product {} has {} items in inventory", product.name, product.qty);
/// }
/// ```
///
/// [`IndexMap`]: std::collections::IndexMap
/// [`Join`]: super::Join
/// [`Query::with_join()`]: crate::sql::query::Query::with_join
impl<T: DataSource, E: Entity> Table<T, E> {
    pub fn with_join<E3: Entity, E2: Entity>(
        mut self,
        their_table: Table<T, E2>,
        our_foreign_id: &str,
    ) -> Table<T, E3> {
        //! Mutate self with a join to another table.
        //!
        //! See [Table::add_join] for more details.
        //!
        //! # Example
        //! Example of creating two Table instances on the fly and then joining them:
        //!
        //! ```rust
        //! let users = Table::new("users", db)
        //!     .with_field("name")
        //!     .with_field("role_id");
        //! let roles = Table::new("roles", db)
        //!     .with_field("id")
        //!     .with_field("role_type");
        //!
        //! let user_with_roles = users.with_join(roles, "role_id");
        //! ```
        //!
        //! # Example in Entity Model
        //! More commonly, you would want to perform joins between tables that are already
        //! defined. In this example, we have existing entities for Product. We want to create
        //! a method `with_inventory` that will create a new entity for `ProductInventory`
        //! struct:
        //!
        //! ```
        //! let product = Product::table();                   // Table<Postgres, Product>
        //! let product_inventory = product.with_inventory(); // Table<Postgres, ProductInventory>
        //! ```
        //!
        //! To implement we need to modify entity model definition:
        //!
        //! ```rust
        //! pub trait ProductTable: AnyTable {
        //!     fn with_inventory(self) -> Table<Postgres, ProductInventory>;
        //! }
        //!
        //! impl ProductTable for Table<Postgres, Product> {
        //!     fn with_inventory(self) -> Table<Postgres, ProductInventory> {
        //!         self
        //!             .with_join(
        //!                 Table::new_with_entity("inventory", postgres())
        //!                     .with_alias("i")
        //!                     .with_id_field("product_id")
        //!                     .with_field("stock"),
        //!                 "id",
        //!             )
        //!             .into_entity::<ProductInventory>()
        //!     }
        //! }
        //!
        //! pub trait ProductInventoryTable: RelatedTable<Postgres> {
        //!     fn stock(&self) -> Arc<Field> {
        //!         // can also use search_for_field() here
        //!         self.get_join("i").unwrap().get_field("stock").unwrap()
        //!     }
        //! }
        //! impl ProductInventoryTable for Table<Postgres, ProductInventory> {}
        //! ```

        self.add_join(their_table, our_foreign_id);
        self.into_entity::<E3>()
    }

    pub fn link<E2: Entity>(&mut self, their_table: &mut Table<T, E2>) {}

    pub fn add_join<E2: Entity>(
        &mut self,
        mut their_table: Table<T, E2>,
        our_foreign_id: &str,
    ) -> Arc<Join<T>> {
        //! Combine two tables with 1 to 1 relationship into a single table.
        //!
        //! Left-Joins their_table table and return self. Assuming their_table has set id field,
        //! but we still have to specify foreign key in our own table. For more complex
        //! joins use `join_table` method.
        //! before joining, make sure there are no alias clashes
        todo!();

        /*
        if eq(&*self.table_aliases, &*their_table.table_aliases) {
            panic!(
                "Tables are already joined: {}, {}",
                self.table_name, their_table.table_name
            )
        }

        if their_table
            .table_aliases
            .lock()
            .unwrap()
            .has_conflict(&self.table_aliases.lock().unwrap())
        {
            panic!(
                "Table alias conflict while joining: {}, {}",
                self.table_name, their_table.table_name
            )
        }

        self.table_aliases
            .lock()
            .unwrap()
            .merge(their_table.table_aliases.lock().unwrap().to_owned());

        // Get information about their_table
        let their_table_name = their_table.table_name.clone();
        if their_table.get_table_alias().is_none() {
            let their_table_alias = self
                .table_aliases
                .lock()
                .unwrap()
                .get_one_of_uniq_id(UniqueIdVendor::all_prefixes(&their_table_name));
            their_table.set_alias(&their_table_alias);
        };
        let their_table_id = their_table.id();

        // Give alias to our table as well
        if self.get_table_alias().is_none() {
            let our_table_alias = self
                .table_aliases
                .lock()
                .unwrap()
                .get_one_of_uniq_id(UniqueIdVendor::all_prefixes(&self.table_name));
            self.set_alias(&our_table_alias);
        }
        let their_table_alias = their_table.get_table_alias().as_ref().unwrap().clone();

        let mut on_condition = QueryConditions::on();
        on_condition.add_condition(
            self.get_column(our_foreign_id)
                .ok_or_else(|| anyhow!("Table '{}' has no field '{}'", &self, &our_foreign_id))
                .unwrap()
                .eq(&their_table_id)
                .render_chunk(),
        );

        // Any condition in their_table should be moved into ON condition
        for condition in their_table.conditions.iter() {
            on_condition.add_condition(condition.render_chunk());
        }
        their_table.conditions = Vec::new();

        // Create a join
        let join = JoinQuery::new(
            JoinType::Left,
            crate::sql::query::QuerySource::Table(
                their_table_name,
                Some(their_table_alias.clone()),
            ),
            on_condition,
        );
        self.joins.insert(
            their_table_alias.clone(),
            Arc::new(Join::new(their_table.into_entity(), join)),
        );

        self.get_join(&their_table_alias).unwrap()
        */
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use serde_json::json;

    use super::*;
    use crate::{
        mocks::datasource::MockDataSource,
        prelude::{Chunk, EmptyEntity, Operations, TableWithQueries},
        sql::Condition,
    };
    #[test]
    fn test_join_1() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let user_table = Table::new("users", db.clone())
            .with_alias("u")
            .with_column("name")
            .with_column("role_id");
        let role_table = Table::new("roles", db.clone())
            .with_column("id")
            .with_column("role_description");

        let table = user_table.with_join::<EmptyEntity, _>(role_table, "role_id");

        let query = table.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT u.name, u.role_id, r.id AS r_id, r.role_description AS r_role_description FROM users AS u LEFT JOIN roles AS r ON (u.role_id = r.id)"
        );
    }

    #[ignore = "broken for now TODO fix"]
    #[test]
    fn join_table_with_joins() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let person = Table::new("person", db.clone())
            .with_column("id")
            .with_column("name")
            .with_column("parent_id");

        let father = person.clone().with_alias("father");
        let grandfather = person.clone().with_alias("grandfather");

        let person = person.with_join::<EmptyEntity, EmptyEntity>(
            father.with_join(grandfather, "parent_id"),
            "parent_id",
        );

        let query = person.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT p.id, p.name, p.parent_id, \
            father.id AS father_id, father.name AS father_name, father.parent_id AS father_parent_id, \
            grandfather.id AS grandfather_id, grandfather.name AS grandfather_name, grandfather.parent_id AS grandfather_parent_id \
            FROM person AS p \
            LEFT JOIN person AS father ON (p.parent_id = father.id) \
            LEFT JOIN person AS grandfather ON (father.parent_id = grandfather.id)"
        );
    }

    #[test]
    fn test_condition_on_joined_table_field() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let mut user_table = Table::new("users", db.clone())
            .with_column("name")
            .with_column("role_id");
        let role_table = Table::new("roles", db.clone())
            .with_column("id")
            .with_column("role_type");

        let join = user_table.add_join(role_table, "role_id");

        user_table.add_condition(join.get_column("role_type").unwrap().eq(&json!("admin")));

        let query = user_table.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT u.name, u.role_id, r.id AS r_id, r.role_type AS r_role_type FROM users AS u LEFT JOIN roles AS r ON (u.role_id = r.id) WHERE (r.role_type = {})"
        );
        assert_eq!(query.1[0], json!("admin"));
    }

    #[test]
    fn test_conditions_moved_into_on() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let mut user_table = Table::new("users", db.clone())
            .with_column("name")
            .with_column("role_id");
        let mut role_table = Table::new("roles", db.clone())
            .with_column("id")
            .with_column("role_type");

        role_table.add_condition(
            role_table
                .get_column("role_type")
                .unwrap()
                .eq(&json!("admin")),
        );

        user_table.add_join(role_table, "role_id");

        let query = user_table.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT u.name, u.role_id, r.id AS r_id, r.role_type AS r_role_type FROM users AS u LEFT JOIN roles AS r ON (u.role_id = r.id) AND (r.role_type = {})"
        );
        assert_eq!(query.1[0], json!("admin"));
    }

    #[test]
    fn test_nested_conditions_moved_into_on() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let mut user_table = Table::new("users", db.clone())
            .with_column("name")
            .with_column("role_id");
        let mut role_table = Table::new("roles", db.clone())
            .with_column("id")
            .with_column("role_type");

        role_table.add_condition(Condition::or(
            role_table
                .get_column("role_type")
                .unwrap()
                .eq(&json!("admin")),
            role_table
                .get_column("role_type")
                .unwrap()
                .eq(&json!("writer")),
        ));

        user_table.add_join(role_table, "role_id");

        let query = user_table.get_select_query().render_chunk().split();

        // TODO: due to Condition::or() implementation, it renders second argument
        // into expression. In fact we push our luck here - perhaps the field we
        // are recursively changing is not even of our table.
        //
        // Ideally table alias should be set before a bunch of Fields are given away
        assert_eq!(
            query.0,
            "SELECT u.name, u.role_id, r.id AS r_id, r.role_type AS r_role_type FROM users AS u \
            LEFT JOIN roles AS r ON (u.role_id = r.id) AND \
            ((r.role_type = {}) OR (role_type = {}))"
        );
        assert_eq!(query.1[0], json!("admin"));
    }

    #[test]
    #[should_panic]
    fn test_join_panic() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let user_table = Table::new("users", db.clone()).with_alias("u");
        let role_table = Table::new("roles", db.clone()).with_alias("u");

        // will panic, both tables want "u" alias
        user_table.with_join::<EmptyEntity, _>(role_table, "role_id");
    }
}
