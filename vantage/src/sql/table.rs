//! [`Table`] struct and various traits for creating SQL table abstractions
//!
//! Separating persistence from business logic requires the ability of a framework to
//! map SQL tables to Rust structs. [`Table`] implements this functionality and
//! is probably the most popular struct in this framework.
//!
//! While [`Table`] represents an SQL table it also implements [`ReadableDataSet`] and
//! [`WritableDataSet`] traits, which means you can easily operate with matching records.
//!
//! The functionality of [`Table`] is split into several areas:
//!  - columns - ability to define physical and logical columns, which is a distinct characteristic of an SQL table
//!  - conditions - ability to narrow scope of a DataSet your table represents
//!  - refs - ability to address related DataSets from your current table and its conditions
//!  - joins - ability to store entity data across multiple tables (could be moved to a separate struct)
//!  - queries - ability to generate [`AssociatedQuery`] (such as [`count()`] or [`sum()`]) from your current table and its conditions
//!  - fetching - ability to interract with the table as a [`ReadableDataSet`] and [`WritableDataSet`]
//!
//! For more information on how to use [`Table`] see <https://romaninsh.github.io/vantage/1-table-and-fields.html>
//!
//! [`ReadableDataSet`]: crate::dataset::ReadableDataSet
//! [`WritableDataSet`]: crate::dataset::WritableDataSet
//! [`count()`]: Table::count()
//! [`sum()`]: Table::sum()

use std::any::{type_name, Any};
use std::fmt::{Debug, Display};
use std::ops::Deref;
use std::sync::{Arc, Mutex};

pub mod column;
mod join;

pub use column::Column;
use column::SqlColumn;
pub use extensions::{Hooks, SoftDelete, TableExtension};
pub use join::Join;

use crate::expr_arc;
use crate::lazy_expression::LazyExpression;
use crate::prelude::{AssociatedQuery, Expression};
use crate::sql::Condition;
use crate::sql::ExpressionArc;
use crate::sql::Query;
use crate::traits::datasource::DataSource;
use crate::traits::entity::{EmptyEntity, Entity};
use crate::uniqid::UniqueIdVendor;
use anyhow::Result;
use indexmap::IndexMap;
use reference::RelatedSqlTable;
use serde_json::{Map, Value};

/// When defining references between tables, AnyTable represents
/// a target table, that can potentially be associated with a
/// different data source.
///
/// The implication is that reference columns need to be explicitly
/// fetched and resulting set of "id"s can then be used to define
/// related queries.
///
/// Table::has_unrelated() can be used to define relation like this.
/// To traverse the relation, use Table::get_unrelated_ref("relation") or
/// Table::get_unrelated_ref_as<D: DataSource, E: Entity>(ds, "relation").
///
/// If tables are defined in the same data source, use has_one(),
/// has_many(), which rely on RelatedTable trait.
///
pub trait AnyTable: Any + Send + Sync {
    fn as_any(self) -> Box<dyn Any>;

    fn as_any_ref(&self) -> &dyn Any;

    fn get_column(&self, name: &str) -> Option<Arc<Column>>;

    fn add_condition(&mut self, condition: Condition);
    fn hooks(&self) -> &Hooks;
}

/// When defining references between tables, RelatedTable represents
/// a target table, that resides in the same DataSource and
/// therefore can be referenced inside a query without explicitly
/// fetching the "id"s.
///
///
pub trait RelatedTable<T: DataSource>: SqlTable {
    fn column_query(&self, column: Arc<Column>) -> AssociatedQuery<T, EmptyEntity>;
    fn add_columns_into_query(&self, query: Query, alias_prefix: Option<&str>) -> Query;
    fn get_join(&self, table_alias: &str) -> Option<Arc<Join<T>>>;

    fn get_alias(&self) -> Option<&String>;
    fn get_table_name(&self) -> Option<&String>;

    fn set_alias(&mut self, alias: &str);

    fn get_columns(&self) -> &IndexMap<String, Arc<Column>>;
    fn get_title_column(&self) -> Option<String>;
}

/// Generic implementation of SQL table.
///
/// Represents a table in the SQL DataSource (potentially with joins and sub-queries)
///
/// ```
/// use vantage::prelude::*;
///
/// let users = Table::new("users", postgres())
///     .with_id_column("id")
///     .with_column("name")
/// ```
///
/// To avoid repetition when defining a table, use Entity definition pattern as described in
/// <https://romaninsh.github.io/vantage/5-entity-model.html>
///
/// # Deciding on `add_` vs `with_` method use
///
/// It is recommended to use `with_` methods for building a table, however `add_` methods are
/// available and will require you to use mutable table reference

#[derive(Debug)]
pub struct Table<T: DataSource, E: Entity> {
    data_source: T,
    _phantom: std::marker::PhantomData<E>,

    table_name: String,
    table_alias: Option<String>,
    id_column: Option<String>,
    title_column: Option<String>,

    conditions: Vec<Condition>,
    columns: IndexMap<String, Arc<Column>>,
    joins: IndexMap<String, Arc<Join<T>>>,
    lazy_expressions: IndexMap<String, LazyExpression<T, E>>,
    refs: IndexMap<String, Arc<Box<dyn RelatedSqlTable>>>,
    table_aliases: Arc<Mutex<UniqueIdVendor>>,

    hooks: Hooks,
}

mod with_columns;
pub use with_columns::TableWithColumns;
pub use with_queries::TableWithQueries;

use super::Chunk;

mod with_joins;
mod with_queries;

mod reference;
mod with_refs;

mod with_updates;

mod with_fetching;

mod extensions;

pub trait SqlTable: TableWithColumns + TableWithQueries {}

impl<T: DataSource, E: Entity> SqlTable for Table<T, E> {}

impl<T: DataSource + Clone, E: Entity> Clone for Table<T, E> {
    fn clone(&self) -> Self {
        Table {
            data_source: self.data_source.clone(),
            _phantom: self._phantom.clone(),

            table_name: self.table_name.clone(),
            table_alias: self.table_alias.clone(),
            id_column: self.id_column.clone(),
            title_column: self.title_column.clone(),

            conditions: self.conditions.clone(),
            columns: self.columns.clone(),
            joins: self.joins.clone(),
            lazy_expressions: self.lazy_expressions.clone(),
            refs: self.refs.clone(),

            // Perform a deep clone of the UniqueIdVendor
            table_aliases: Arc::new(Mutex::new((*self.table_aliases.lock().unwrap()).clone())),

            hooks: self.hooks.clone(),
        }
    }
}

impl<T: DataSource, E: Entity> Display for Table<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{}> {{ name={} }}", type_name::<E>(), self.table_name)
    }
}

impl<T: DataSource, E: Entity> AnyTable for Table<T, E> {
    fn as_any(self) -> Box<dyn Any> {
        Box::new(self)
    }
    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    /// Handy way to reference column by name, for example to use with [`Operations`].
    ///
    /// [`Operations`]: super::super::operations::Operations
    fn get_column(&self, name: &str) -> Option<Arc<Column>> {
        self.columns.get(name).cloned()
    }
    fn add_condition(&mut self, condition: Condition) {
        self.conditions.push(condition);
    }
    fn hooks(&self) -> &Hooks {
        &self.hooks
    }
}

impl<T: DataSource, E: Entity> RelatedTable<T> for Table<T, E> {
    fn column_query(&self, column: Arc<Column>) -> AssociatedQuery<T, EmptyEntity> {
        let query = self.get_empty_query().with_field(column.name(), column);
        AssociatedQuery::new(query, self.data_source.clone())
    }

    // TODO: debug why this overwrites the previous columns
    fn add_columns_into_query(&self, mut query: Query, alias_prefix: Option<&str>) -> Query {
        for (column_key, column_val) in &self.columns {
            let column_val = if let Some(alias_prefix) = &alias_prefix {
                let alias = format!("{}_{}", alias_prefix, column_key);
                let mut column_val = column_val.deref().clone();
                column_val.set_column_alias(alias);
                Arc::new(column_val)
            } else {
                column_val.clone()
            };
            query = query.with_field(
                column_val
                    .deref()
                    .get_column_alias()
                    .unwrap_or_else(|| column_key.clone()),
                column_val,
            );
        }

        for (alias, join) in &self.joins {
            query = join.add_columns_into_query(query, Some(alias));
        }

        query
    }

    fn get_alias(&self) -> Option<&String> {
        self.table_alias.as_ref()
    }
    fn set_alias(&mut self, alias: &str) {
        if let Some(alias) = &self.table_alias {
            self.table_aliases.lock().unwrap().dont_avoid(alias);
        }
        self.table_alias = Some(alias.to_string());
        self.table_aliases.lock().unwrap().avoid(alias);
        for column in self.columns.values_mut() {
            let mut new_column = column.deref().deref().clone();
            new_column.set_table_alias(alias.to_string());
            *column = Arc::new(new_column);
        }
        for condition in &mut self.conditions {
            condition.set_table_alias(alias);
        }
    }
    fn get_table_name(&self) -> Option<&String> {
        Some(&self.table_name)
    }
    fn get_columns(&self) -> &IndexMap<String, Arc<Column>> {
        &self.columns
    }
    fn get_join(&self, table_alias: &str) -> Option<Arc<Join<T>>> {
        self.joins.get(table_alias).map(|r| r.clone())
    }
    fn get_title_column(&self) -> Option<String> {
        self.title_column.clone()
    }
}

impl<T: DataSource, E: Entity> Table<T, E> {
    pub fn new_with_entity(table_name: &str, data_source: T) -> Table<T, E> {
        Table {
            data_source,
            _phantom: std::marker::PhantomData,

            table_name: table_name.to_string(),
            table_alias: None,
            id_column: None,
            title_column: None,

            conditions: Vec::new(),
            columns: IndexMap::new(),
            joins: IndexMap::new(),
            lazy_expressions: IndexMap::new(),
            refs: IndexMap::new(),
            table_aliases: Arc::new(Mutex::new(UniqueIdVendor::new())),

            hooks: Hooks::new(),
        }
    }
}

impl<T: DataSource> Table<T, EmptyEntity> {
    pub fn new(table_name: &str, data_source: T) -> Table<T, EmptyEntity> {
        Table {
            data_source,
            _phantom: std::marker::PhantomData,

            table_name: table_name.to_string(),
            table_alias: None,
            id_column: None,
            title_column: None,

            conditions: Vec::new(),
            columns: IndexMap::new(),
            joins: IndexMap::new(),
            lazy_expressions: IndexMap::new(),
            refs: IndexMap::new(),
            table_aliases: Arc::new(Mutex::new(UniqueIdVendor::new())),

            hooks: Hooks::new(),
        }
    }
}

impl<T: DataSource, E: Entity> Table<T, E> {
    /// Use a callback with a builder pattern:
    /// ```
    /// let books = BookSet::new().with(|b| {
    ///    b.add_column("title");
    ///    b.add_column("price");
    /// }).with(|b| {
    ///    b.add_condition(b.get_column("title").unwrap().gt(100));
    /// });
    /// ```
    pub fn with<F>(mut self, func: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        func(&mut self);
        self
    }

    pub fn into_entity<E2: Entity>(self) -> Table<T, E2> {
        Table {
            data_source: self.data_source,
            _phantom: std::marker::PhantomData,

            table_name: self.table_name,
            table_alias: self.table_alias,
            id_column: self.id_column,
            title_column: self.title_column,

            conditions: self.conditions,
            columns: self.columns,
            joins: self.joins,
            lazy_expressions: IndexMap::new(), // TODO: cast proprely
            refs: IndexMap::new(),             // TODO: cast proprely

            // Perform a deep clone of the UniqueIdVendor
            table_aliases: Arc::new(Mutex::new((*self.table_aliases.lock().unwrap()).clone())),

            hooks: self.hooks,
        }
    }

    pub fn with_alias(mut self, alias: &str) -> Self {
        self.set_alias(alias);
        self
    }

    /// Add a condition to the table, limiting what records
    /// the DataSet will represent
    pub fn add_condition(&mut self, condition: Condition) {
        self.conditions.push(condition);
    }

    /// A handy way to add a condition during table building:
    pub fn with_condition(mut self, condition: Condition) -> Self {
        self.add_condition(condition);
        self
    }

    // ---- Expressions ----
    //  BeforeQuery(Arc<Box<dyn Fn(&Query) -> Expression>>),
    pub fn add_expression(
        &mut self,
        name: &str,
        expression: impl Fn(&Table<T, E>) -> Expression + 'static + Sync + Send,
    ) {
        self.lazy_expressions.insert(
            name.to_string(),
            LazyExpression::BeforeQuery(Arc::new(Box::new(expression))),
        );
    }

    pub fn with_expression(
        mut self,
        name: &str,
        expression: impl Fn(&Table<T, E>) -> Expression + 'static + Sync + Send,
    ) -> Self {
        self.add_expression(name, expression);
        self
    }

    pub fn with_extension(mut self, extension: impl TableExtension + 'static) -> Self {
        extension.init(&mut self);
        self.hooks.add_hook(Box::new(extension));

        self
    }

    pub async fn get_all_data(&self) -> Result<Vec<Map<String, Value>>> {
        self.data_source.query_fetch(&self.get_select_query()).await
    }

    pub fn sum<C>(&self, column: C) -> AssociatedQuery<T, EmptyEntity>
    where
        C: Chunk,
    {
        let query = self.get_empty_query().with_field(
            "sum".to_string(),
            expr_arc!("SUM({})", column.render_chunk()),
        );
        AssociatedQuery::new(query, self.data_source.clone())
    }

    pub fn count(&self) -> AssociatedQuery<T, EmptyEntity> {
        let mut query = self
            .get_empty_query()
            .with_field("count".to_string(), expr_arc!("COUNT(*)"));
        self.hooks().before_select_query(self, &mut query).unwrap();
        AssociatedQuery::new(query, self.data_source.clone())
    }
}

// impl<T: DataSource, E: Entity> WritableDataSet for Table<T, E> {
//     fn insert_query(&self) -> Query {
//         todo!()
//     }

//     fn update_query(&self) -> Query {
//         todo!()
//     }

//     fn delete_query(&self) -> Query {
//         todo!()
//     }
// }

pub trait TableDelegate<T: DataSource, E: Entity>: TableWithColumns {
    fn table(&self) -> &Table<T, E>;

    fn id(&self) -> Arc<Column> {
        self.table().id()
    }
    fn add_condition(&self, condition: Condition) -> Table<T, E> {
        self.table().clone().with_condition(condition)
    }
    fn sum(&self, column: Arc<Column>) -> AssociatedQuery<T, EmptyEntity> {
        self.table().sum(column)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use serde_json::json;

    use super::*;
    use crate::{
        mocks::datasource::MockDataSource,
        prelude::{Chunk, Operations},
    };

    #[tokio::test]
    async fn test_table() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);

        let data_source = MockDataSource::new(&data);

        let table = Table::new("users", data_source.clone())
            .with_column("name")
            .with_column("surname");

        let result = table.get_all_data().await;

        assert_eq!(result.unwrap().clone(), *data_source.data());
    }

    #[tokio::test]
    async fn test_with() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let books = Table::new("book", data_source)
            .with(|b| {
                b.add_column("title".to_string(), Column::new("title".to_string(), None));
                b.add_column("price".to_string(), Column::new("price".to_string(), None));
            })
            .with(|b| {
                b.add_condition(b.get_column("title").unwrap().gt(100));
            });

        let query = books.get_select_query().render_chunk().split();

        assert_eq!(query.0, "SELECT title, price FROM book WHERE (title > {})");
    }

    #[tokio::test]
    async fn test_conditions() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let data_source = MockDataSource::new(&data);

        let mut table = Table::new("users", data_source.clone())
            .with_column("name")
            .with_column("surname");

        table.add_condition(table.get_column("name").unwrap().eq(&"John".to_string()));

        let query = table.get_select_query().render_chunk().split();

        assert_eq!(query.0, "SELECT name, surname FROM users WHERE (name = {})");
        assert_eq!(query.1[0], json!("John"));

        let result = table.get_all_data().await;

        assert_eq!(result.unwrap(), *data_source.data());
    }

    #[test]
    fn test_vip_client() {
        let data =
            json!([{ "name": "John", "surname": "Doe"}, { "name": "Jane", "surname": "Doe"}]);
        let db = MockDataSource::new(&data);

        let mut vip_client = Table::new("client", db)
            .with_title_column("name")
            .with_column("is_vip")
            .with_column("total_spent");

        vip_client.add_condition(vip_client.get_column("is_vip").unwrap().eq(&true));

        let sum = vip_client.sum(vip_client.get_column("total_spent").unwrap());
        assert_eq!(
            sum.render_chunk().sql().clone(),
            "SELECT (SUM(total_spent)) AS sum FROM client WHERE (is_vip = {})".to_owned()
        );
    }
}
