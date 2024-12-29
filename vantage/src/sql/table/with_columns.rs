use anyhow::{anyhow, Context};
use indexmap::IndexMap;
use serde_json::Value;
use std::sync::Arc;

use super::column::SqlColumn;
use super::{PgValueColumn, SqlTable};
use crate::expr;
use crate::lazy_expression::LazyExpression;
use crate::prelude::Operations;
use crate::sql::table::Table;
use crate::sql::Expression;
use crate::traits::column::SqlField;
use crate::traits::datasource::DataSource;
use crate::traits::entity::Entity;

use super::AnyTable;

/// # Table Columns
///
/// Unlike a [`Query`], the [`Table`] will have a fixed set of columns, that you
/// should define and later reference. Typically column structure is defined in
/// your Entity model definition: <https://romaninsh.github.io/vantage/5-entity-model.html>,
/// however in the examples below, we will define the columns ad-hoc.
///
/// ```
/// use vantage::prelude::*;
///
/// let users = Table::new("users", postgres())
///     .with_id_column("id")
///     .with_title_column("name")
///     .with_column("role_name");
///
/// let some_users = users.clone()
///     .with_condition(users.get_column("role_name").unwrap().eq("admin"));
/// ```
///
/// ## Adding and referencing columns:
///
/// Any column you add to the table can be retrieved with [`Table::get_column()`] method.
/// Method actually returns [`Option`]<[`Arc`]<[`Column`]>>, but you should keep `Column`
/// wrapped in `Arc` to avoid cloning it.
///
/// It is a common practice to define a business entity model and use it along with table.
/// Here is a simplified example of defining `Table<Postgress, User>`.
///
/// ```
/// struct User {
///     id: i32,
///     name: String,
///     role_name: String,
/// }
/// impl User {
///     fn table() -> Table<Postgres, User> {
///         Table::new_with_entity("user", postgres())
///             .with_id_column("id")
///             .with_title_column("name")
///             .with_column("role_name")
///     }
/// }
/// pub trait UserTable: AnyTable {
///     fn role_name(&self) -> Arc<Column> {
///         self.get_column("role_name").unwrap()
///     }
/// }
/// impl UserTable for Table<Postgres, User> {}
/// ```
///
/// The above code defines a useful method: `User::table()` that returns a `Table<Postgres, User>`.
/// By implementing additional trait on `Table<Postgres, User>` you can effectively combine
/// existing methods from `Table` with your own methods:
///
/// ```
/// let users = User::table();
/// dbg!(users.get_column("role_name")); // Some(Arc<Column>)
/// dbg!(users.role_name());            // Arc<Column>
/// ```
///
/// A method `role_name()` can perform the unwrap, since we can be sure that such a column exists.
///
/// ```
/// let admin_users = User::table().
///     .with_condition(User::table().role_name().eq("admin"));
/// ```
///
/// ## `id` and `title` columns:
///
/// Methods [`with_id_column()`] and [`with_title_column()`] are identical to [`with_column()`]
/// but they will set internal reference to a column.
///
/// Using [`id()`] can be used to reference the `id` column more conveniently. Also [`with_id`]
/// can be used to set condition on the `id` column. You should always define some column as `id`
/// column, when creating a table.
///
/// ## Advanced column definition:
///
/// When you use [`with_column()`] method, it automatically creates a [`Column`] object
/// and adds it into an internal `columns` [`IndexMap`]. By specifying [`Column`] directly,
/// you can provide additional information about the column, such as alias.
///
/// Method [`add_column()`] accepts a [`Column`] as a second argument and you might want
/// to use it on some rare occasions.
///
/// You may access the internal `columns` [`IndexMap`] with [`columns()`] method.
///
///
/// [`Query`]: super::Query
/// [`with_id_column()`]: Table::with_id_column()
/// [`with_title_column()`]: Table::with_title_column()
/// [`with_column()`]: Table::with_column()
/// [`add_column()`]: Table::add_column()
/// [`id()`]: Table::id()
/// [`with_id`]: Table::with_id()
/// [`columns()`]: Table::columns()

pub trait TableWithColumns: AnyTable {
    fn add_column(&mut self, column_name: String, column: PgValueColumn);
    fn columns(&self) -> &IndexMap<String, Arc<PgValueColumn>>;
    // fn get_column_with_table_alias(&self, name: &str) -> Option<Arc<PgValueColumn>>;
    fn id(&self) -> Arc<PgValueColumn>;
    // fn id_with_table_alias(&self) -> Arc<PgValueColumn>;
    fn search_for_field(&self, field_name: &str) -> Option<Box<dyn SqlField>>;
}

impl<T: DataSource, E: Entity> TableWithColumns for Table<T, E> {
    /// **avoid using directly**.
    ///
    /// Adds a new column to the table. Note, that Column may use an alias. Additional
    /// features may be added into [`Column`] in the future, so better use [`with_column()`]
    /// to keep your code portable.
    fn add_column(&mut self, column_name: String, mut column: PgValueColumn) {
        column.set_table_alias(&self.alias);
        self.columns.insert(column_name, Arc::new(column));
    }

    /// Return all columns. See also: [`Table::get_column`].
    fn columns(&self) -> &IndexMap<String, Arc<PgValueColumn>> {
        &self.columns
    }

    // fn get_column_with_table_alias(&self, name: &str) -> Option<Arc<PgValueColumn>> {
    //     Some(Arc::new(self.get_column(name)?.with_table_alias()))
    // }
    // fn id_with_table_alias(&self) -> Arc<PgValueColumn> {
    //     Arc::new(self.id().with_table_alias())
    // }

    /// Returns the id column. If `with_id_column` was not called, will try to find
    /// column called `"id"`. If not found, will panic.
    fn id(&self) -> Arc<PgValueColumn> {
        let id_column = if self.id_column.is_some() {
            let x = self.id_column.clone().unwrap();
            x.clone()
        } else {
            "id".to_string()
        };
        self.get_column(&id_column)
            .with_context(|| anyhow!("Table '{}' has no field '{}'", &self, &id_column))
            .unwrap()
    }

    // fn id_with_table_alias(&self) -> Arc<PgValueColumn> {
    //     let id_column = if self.id_column.is_some() {
    //         let x = self.id_column.clone().unwrap();
    //         x.clone()
    //     } else {
    //         "id".to_string()
    //     };
    //     self.get_column_with_table_alias(&id_column).unwrap()
    // }

    /// In addition to `self.columns` the columns can also be defined for a joined
    /// table. (See [`Table::with_join()`]) or through a lazy expression (See
    /// [`Table::with_expression()`]).
    ///
    /// The more broad scope requires us to use a [`Column`] trait rather than
    /// a [`Column`].
    ///
    /// [`Column`]: vantage::sql::Column
    fn search_for_field(&self, field_name: &str) -> Option<Box<dyn SqlField>> {
        // perhaps we have a field like this?
        if let Some(column) = self.get_column(field_name) {
            return Some(Box::new(column));
        }

        // maybe joined table have a field we want
        for (_, join) in self.joins.iter() {
            if let Some(column) = join.table().get_column(field_name) {
                return Some(Box::new(column.clone()));
            }
        }

        // maybe we have a lazy expression
        if let Some(lazy_expression) = self.lazy_expressions.get(field_name) {
            return match lazy_expression {
                LazyExpression::AfterQuery(_) => None,
                LazyExpression::BeforeQuery(expr) => {
                    let x = (expr)(self);
                    Some(Box::new(x))
                }
            };
        }
        None
    }
}

impl<T: DataSource, E: Entity> Table<T, E> {
    /// When building a table - a way to chain column declarations.
    pub fn with_column(mut self, column: impl Into<PgValueColumn>) -> Self {
        let column: PgValueColumn = column.into();
        // if self.table_alias.is_some() {
        //     column.set_table_alias(self.table_alias.clone().unwrap());
        // }
        self.add_column(column.name(), column);
        self
    }

    /// Adds a column that is also a title column. Title column will be
    /// used in the UI to represent the record.
    pub fn with_title_column(mut self, column: &str) -> Self {
        self.title_column = Some(column.to_string());
        self.with_column(column)
    }

    /// Adds a column that is also an id column. Id column is used
    /// by [`Table::id()`] and [`Table::with_id()`].
    pub fn with_id_column(mut self, column: &str) -> Self {
        self.id_column = Some(column.to_string());
        self.with_column(column)
    }

    /// Will add a condition for the `id` column. This is a syntactic sugar for
    /// `with_condition(id().eq(&id))`.
    pub fn with_id(self, id: Value) -> Self {
        let f = self.id().eq(&id);
        self.with_condition(f)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{mocks::datasource::MockDataSource, prelude::*, sql::table::Table};

    #[test]
    fn test_get_column() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let roles = Table::new("roles", db.clone())
            .with_column("id")
            .with_column("name");

        assert!(roles.get_column("qq").is_none());
        assert!(roles.get_column("name").is_some());
    }

    #[test]
    fn test_add_column() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let roles = Table::new("roles", db.clone())
            .with_column(PgValueColumn::new("name").with_alias("id"));

        assert!(roles.get_column("qq").is_none());
        assert!(roles.get_column("name").is_some());
    }

    #[test]
    fn test_search_for_field() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let roles = Table::new("roles", db.clone())
            .with_column("id")
            .with_column("name")
            .with_expression("surname", |_| expr!("foo"));

        assert!(roles.search_for_field("qq").is_none());
        assert!(roles.search_for_field("name").is_some());
        assert!(roles.search_for_field("surname").is_some());
        assert!(roles.get_column("surname").is_none())
    }

    #[test]
    fn test_column_query() {
        let data = json!([]);
        let db = MockDataSource::new(&data);

        let mut roles = Table::new("roles", db.clone())
            .with_column("id")
            .with_column("name");

        roles.add_condition(roles.get_column("name").unwrap().eq(&"admin".to_string()));
        let query = roles.field_query(roles.get_column("id").unwrap());

        assert_eq!(
            query.render_chunk().sql().clone(),
            "SELECT id FROM roles WHERE (name = {})".to_owned()
        );

        let mut users = Table::new("users", db.clone())
            .with_column("id")
            .with_column("role_id");

        users.add_condition(users.get_column("role_id").unwrap().in_expr(&query));
        let query = users.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT id, role_id FROM users WHERE (role_id IN (SELECT id FROM roles WHERE (name = {})))"
        );
        assert_eq!(query.1[0], json!("admin"));
    }
}
