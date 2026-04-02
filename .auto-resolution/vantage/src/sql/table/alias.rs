use crate::{traits::DataSource, uniqid::UniqueIdVendor};
use anyhow::{Context, Result, anyhow};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio_postgres::types::ToSql;

use super::{Join, SqlTable};

/// For a table (in a wider join) describes how the table should be aliased.
/// AutoAssigned alias can be automatically changed to resolve conflicts. Explicitly
/// requesting alias will not be changed automatically, but can be changed manually.
/// None would never attempt to alias a table (uses table name) and Any will be
/// automatically changed to AutoAssigned() when a conflict arises.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DesiredAlias<T> {
    /// Never use any alias
    None,
    /// Use any alias
    Any,
    /// Use alias that was auto-assigned
    AutoAssigned(T),
    /// Use explicitly requested alias
    ExplicitlyRequested(T),
}
impl<T> DesiredAlias<T> {
    pub fn unwrap(&self) -> &T {
        match self {
            DesiredAlias::None => panic!("Alias is disabled"),
            DesiredAlias::Any => panic!("Alias was not set yet"),
            DesiredAlias::AutoAssigned(t) => t,
            DesiredAlias::ExplicitlyRequested(t) => t,
        }
    }
    pub fn is_some(&self) -> bool {
        match self {
            DesiredAlias::None => false,
            DesiredAlias::Any => false,
            _ => true,
        }
    }
}

impl<T> Into<Option<T>> for DesiredAlias<T> {
    fn into(self) -> Option<T> {
        match self {
            DesiredAlias::None => None,
            DesiredAlias::Any => None,
            DesiredAlias::AutoAssigned(t) => Some(t),
            DesiredAlias::ExplicitlyRequested(t) => Some(t),
        }
    }
}

/// TableAlias is a shareable configuration for table alias configuration,
/// accessible by columns of that table.
///
/// Columns can request table alias configuration at any time through
/// table_alias.try_get() (equal to read().unwrap().try_get()) which returns
/// optional table alias. This will inform column if it should prefix
/// itself with a table.
///
/// Column may want to explicitly use a table alias through table_alias.get(),
/// which will return either table alias or table name.
///
/// If Table makes use of joins, the table aliases will be automatically re-assigned
/// and enforced for all columns, even if column was cloned earlier.
///
/// Cloning the table will deep-clone alias config and therefore will have
/// independent alias settings
///
/// Joining tables does merges unique id vendor across all tables, and all
/// alias configuration will be set to enforce alias.

#[derive(Debug, Clone)]
struct TableAliasConfig {
    // Copy of a table name
    table_name: String,

    // User requested for table to have a custom alias
    desired_alias: DesiredAlias<String>,

    // Should we include table(or alias) when rendering field queries (e.g. select user.name from user)
    specify_table_for_field_queries: bool,

    // ID generated shared by all joined tables to re-generate DesiredAlias::AutoAssigned<?>
    alias_vendor: Arc<Mutex<UniqueIdVendor>>,
}

impl TableAliasConfig {
    pub fn new(table_name: &str) -> Self {
        let mut id_vendor = UniqueIdVendor::new();
        let alias = id_vendor.avoid(table_name);

        TableAliasConfig {
            table_name: table_name.to_string(),
            desired_alias: DesiredAlias::Any,
            specify_table_for_field_queries: false,
            alias_vendor: Arc::new(Mutex::new(id_vendor)),
        }
    }

    /// When our table joins other tables, we enforce table prefix for all columns.
    /// This is important because columns can be ambiguous. In addition to setting
    /// the flag, we will also auto-generate alias if DesiredAlias is set to Any
    pub fn enforce_table_in_field_queries(&mut self) {
        if self.specify_table_for_field_queries {
            return;
        }
        if self.desired_alias == DesiredAlias::Any {
            // we will treat table name as alias now, so we must reserve it
            // let t = self.table_name.clone();
            // self.set(&t);
        }
        self.specify_table_for_field_queries = true;
    }

    /// Use custom alias for this table. If alias was used previously, it won't be reserved
    /// anymore.
    pub fn set(&mut self, alias: &str) {
        // If alias is ExplicitlyRequested or AutoAssigned, we must release it
        if self.desired_alias.is_some() {
            self.alias_vendor
                .lock()
                .unwrap()
                .dont_avoid(self.desired_alias.unwrap())
                .unwrap();
        }
        let alias = self.alias_vendor.lock().unwrap().get_uniq_id(alias);
        self.desired_alias = DesiredAlias::ExplicitlyRequested(alias.to_string());
    }

    pub fn set_short_alias(&mut self) {
        self.desired_alias = DesiredAlias::AutoAssigned(
            self.alias_vendor
                .lock()
                .unwrap()
                .get_short_uniq_id(&self.table_name),
        )
    }

    /// Used by a column if it wants to be explicitly prefixed (e.g. used in subquery)
    pub fn get(&self) -> String {
        if self.desired_alias.is_some() {
            self.desired_alias.unwrap().clone()
        } else {
            self.table_name.clone()
        }
    }

    pub fn alias_is_some(&self) -> bool {
        self.desired_alias.is_some()
    }

    /// Used by a column natively, to guard against situations when we join more tables
    /// and therefore all fields should be prefixed to avoid ambiguity
    pub fn try_get(&self) -> Option<String> {
        if self.specify_table_for_field_queries {
            Some(self.get())
        } else {
            None
        }
    }

    /// Used for FROM field to append "AS"
    pub fn try_get_for_from(&self) -> Option<String> {
        if self.desired_alias.is_some() {
            Some(self.get())
        } else {
            None
        }
    }

    pub fn deep_clone(&self) -> Self {
        TableAliasConfig {
            table_name: self.table_name.clone(),
            desired_alias: self.desired_alias.clone(),
            specify_table_for_field_queries: self.specify_table_for_field_queries,
            alias_vendor: Arc::new(Mutex::new(UniqueIdVendor::new())),
        }
    }

    /// Get rid of existing ID vendor, and replace with a clone of the one
    /// we are providing. Subsequently you will need to lock alias with
    /// _lock_explicit_alias and _lock_implicit_alias
    pub fn _reset_id_vendor(&mut self, id_vendor: Arc<Mutex<UniqueIdVendor>>) {
        self.alias_vendor = id_vendor;
    }

    /// Assuming that uniq id vendor was set but not initialized yet with
    /// our table - reserve explicit our explicit alias (if we have it)
    pub fn _lock_explicit_alias(&mut self) -> Result<()> {
        match &self.desired_alias {
            DesiredAlias::ExplicitlyRequested(a) => self.alias_vendor.lock().unwrap().avoid(a)?,
            DesiredAlias::None => self.alias_vendor.lock().unwrap().avoid(&self.table_name)?,
            _ => {}
        }
        Ok(())
    }

    /// After all tables have their explicit aliases locked in, we will do
    /// another pass calculating auto-assigned aliases. The logic here would
    /// be to use shortened table name (e.g. p for person) but append _1, _2
    /// if it clashes with similar tables.
    pub fn _lock_implicit_alias(&mut self) {
        match &self.desired_alias {
            DesiredAlias::ExplicitlyRequested(_) => return,
            DesiredAlias::None => return,
            _ => {
                self.desired_alias = DesiredAlias::AutoAssigned(
                    self.alias_vendor
                        .lock()
                        .unwrap()
                        .get_short_uniq_id(&self.table_name),
                )
            }
        }
    }

    pub fn _reassign_alias<TT: DataSource>(
        &mut self,
        our_old_joins: IndexMap<String, Arc<Join<TT>>>,
        their_old_joins: IndexMap<String, Arc<Join<TT>>>,
    ) -> Result<IndexMap<String, Arc<Join<TT>>>> {
        let mut result = IndexMap::new();

        let tmp: Vec<_> = our_old_joins
            .into_values()
            .chain(their_old_joins.into_values())
            .map(|j| j.split())
            .collect();

        self.alias_vendor = Arc::new(Mutex::new(UniqueIdVendor::new()));

        for (table, _) in &tmp {
            table
                .alias
                .config
                .write()
                .unwrap()
                ._reset_id_vendor(self.alias_vendor.clone());
        }

        self._lock_explicit_alias()
            .context(anyhow!("for primary table"))?;

        for (table, _) in &tmp {
            table.alias.config.write().unwrap()._lock_explicit_alias()?;
        }

        self._lock_implicit_alias();

        for (table, join_query) in tmp {
            table.alias.config.write().unwrap()._lock_implicit_alias();

            let alias = table.alias.get();
            result.insert(alias, Arc::new(Join::new(table, join_query)));
        }

        Ok(result)
    }
}

#[derive(Clone, Debug)]
pub struct TableAlias {
    config: Arc<RwLock<TableAliasConfig>>,
}

impl TableAlias {
    pub fn new(table_name: &str) -> Self {
        TableAlias {
            config: Arc::new(RwLock::new(TableAliasConfig::new(table_name))),
        }
    }
    pub fn enforce_table_in_field_queries(&self) {
        self.config
            .write()
            .unwrap()
            .enforce_table_in_field_queries();
    }
    pub fn try_get(&self) -> Option<String> {
        self.config.read().unwrap().try_get()
    }
    pub fn try_get_for_from(&self) -> Option<String> {
        self.config.read().unwrap().try_get_for_from()
    }
    pub fn get(&self) -> String {
        self.config.read().unwrap().get()
    }
    pub fn alias_is_some(&self) -> bool {
        self.config.read().unwrap().alias_is_some()
    }
    pub fn set(&self, alias: &str) {
        self.config.write().unwrap().set(alias);
    }
    pub fn set_short_alias(&self) {
        self.config.write().unwrap().set_short_alias();
    }
    pub fn disable_alias(&self) {
        self.config.write().unwrap().desired_alias = DesiredAlias::None;
    }
    pub fn deep_clone(&self) -> Self {
        Self {
            config: Arc::new(RwLock::new(self.config.read().unwrap().deep_clone())),
        }
    }
    /// Returns true if both table alias records have same vendor ID
    /// which effectively mean the tables are joined
    pub fn is_same_id_vendor(&self, other: &Self) -> bool {
        Arc::ptr_eq(
            &self.config.read().unwrap().alias_vendor,
            &other.config.read().unwrap().alias_vendor,
        )
    }

    pub fn _reassign_alias<TT: DataSource>(
        &self,
        our_old_joins: IndexMap<String, Arc<Join<TT>>>,
        their_old_joins: IndexMap<String, Arc<Join<TT>>>,
    ) -> Result<IndexMap<String, Arc<Join<TT>>>> {
        self.config
            .write()
            .unwrap()
            ._reassign_alias(our_old_joins, their_old_joins)
    }
}

#[cfg(test)]
mod tests {
    use std::{os, sync::Arc};

    use crate::{
        expr_arc,
        prelude::{ExpressionArc, JoinQuery, PgValueColumn, SqlTable, TableWithQueries},
        sql::{
            Chunk, Join,
            query::{ConditionType, JoinType, QueryConditions, QuerySource},
        },
    };
    use indexmap::IndexMap;
    use serde_json::json;

    use crate::{mocks::MockDataSource, prelude::AnyTable, sql::Table};

    #[test]
    fn test_table_cloning() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let table = Table::new("users", data_source.clone()).with_column("name");

        let table2 = table.clone();

        assert_eq!(
            table
                .alias
                .config
                .read()
                .unwrap()
                .specify_table_for_field_queries,
            false
        );
        assert_eq!(
            table2
                .alias
                .config
                .read()
                .unwrap()
                .specify_table_for_field_queries,
            false
        );

        table.alias.enforce_table_in_field_queries();

        assert_eq!(
            table
                .alias
                .config
                .read()
                .unwrap()
                .specify_table_for_field_queries,
            true
        );
        assert_eq!(
            table2
                .alias
                .config
                .read()
                .unwrap()
                .specify_table_for_field_queries,
            false
        );
    }

    #[test]
    fn test_reassign_alias() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let table = Table::new("users", data_source.clone()).with_column("name");

        let table1 = table.clone();
        let mut table2 = table.clone();
        let table3 = table.clone();
        let table4 = table.clone();

        // leave table1 as-is
        table2.set_alias("uzzah");
        table3.alias.disable_alias();
        // leave table4 as-is

        let some_join_query = JoinQuery::new(
            JoinType::Inner,
            QuerySource::Table("user".to_string(), None),
            QueryConditions::on(),
        );

        let mut i1 = IndexMap::new();
        i1.insert(
            "a".to_string(),
            Arc::new(Join::new(table1, some_join_query.clone())),
        );
        i1.insert(
            "b".to_string(),
            Arc::new(Join::new(table2, some_join_query.clone())),
        );

        let mut i2 = IndexMap::new();
        i2.insert(
            "c".to_string(),
            Arc::new(Join::new(table3, some_join_query.clone())),
        );
        i2.insert(
            "d".to_string(),
            Arc::new(Join::new(table4, some_join_query.clone())),
        );

        // after merging all tables, we should have u_1, uzzah, users, u_2
        let result = table.alias._reassign_alias(i1, i2).unwrap();

        // Resulting join uses `user` table 5 times.
        //
        // table is as-is and is assetred above, aliased as `u`
        //
        // table1 was as-is, but `u` is taken, so aliased into `us`
        // table2.set_alias("uzzah");
        // table3.alias.disable_alias(), so `users` is used
        // leave table4 as-is, so `use` is the alias
        assert_eq!(&table.alias.config.read().unwrap().get(), "u");
        assert_eq!(
            result
                .iter()
                .map(|(k, t)| (k, t.table_name.clone()))
                .collect::<Vec<_>>(),
            vec![
                (&"us".to_string(), "users".to_string()),
                (&"uzzah".to_string(), "users".to_string()),
                (&"users".to_string(), "users".to_string()),
                (&"use".to_string(), "users".to_string())
            ]
        );
    }

    #[test]
    fn test_try_and_get_alias() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let table = Table::new("users", data_source.clone())
            .with_column(PgValueColumn::new("name").with_quotes());

        // render field regularly
        let f1 = table.get_column("name").unwrap().with_quotes();
        assert_eq!(f1.render_chunk().preview(), "\"name\"");

        // get field with table alias
        let f2 = table.get_column("name").unwrap().with_table_alias();
        assert_eq!(f2.render_chunk().preview(), "\"users\".\"name\"");

        // next change table alias and make sure existing fields are affected
        table.alias.set("u");
        assert_eq!(expr_arc!("{}", f1.render_chunk()).preview(), "\"name\"");
        assert_eq!(
            expr_arc!("{}", f2.render_chunk()).preview(),
            "\"u\".\"name\""
        );

        // setting enforce will prefix all fields with table name or table alias
        table.alias.enforce_table_in_field_queries();
        assert_eq!(
            expr_arc!("{}", f1.render_chunk()).preview(),
            "\"u\".\"name\""
        );
        assert_eq!(
            expr_arc!("{}", f2.render_chunk()).preview(),
            "\"u\".\"name\""
        );
    }

    #[test]
    fn test_table_joining() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let mut person = Table::new("person", data_source.clone()).with_column("name");
        let mut father = person.clone();

        let person_name = person.get_column("name").unwrap();
        let father_name = father.get_column("name").unwrap();

        // Tables are unrelated, so both names render without alias
        assert_eq!(person_name.render_chunk().preview(), "name");
        assert_eq!(father_name.render_chunk().preview(), "name");

        // Linking father to a person, will enforce table prefixing for both but since
        // the table name is identical, a unique alias will be generated
        person.link(&mut father);
        dbg!(&person.alias);
        return;
        assert_eq!(person_name.render_chunk().preview(), "person.name");
        assert_eq!(
            father_name.render_chunk().preview(),
            "\"person_2\".\"name\""
        );

        father.alias.set("par");
        assert_eq!(person_name.render_chunk().preview(), "person.name");
        assert_eq!(father_name.render_chunk().preview(), "par.name");

        let mut mother = Table::new("person", data_source.clone())
            .with_column("name")
            .with_alias("par");
        let mother_name = mother.get_column("name").unwrap();
        person.link(&mut mother);

        assert_eq!(person_name.render_chunk().preview(), "person.name");
        assert_eq!(father_name.render_chunk().preview(), "par.name");
        assert_eq!(mother_name.render_chunk().preview(), "par_2.name");

        // now lets add grandparents
        let mut gr1 = Table::new("person", data_source.clone()).with_column("name");
        let gr1_name = gr1.get_column("name").unwrap();

        let mut gr2 = Table::new("person", data_source.clone()).with_column("name");
        let gr2_name = gr2.get_column("name").unwrap();

        assert_eq!(gr1_name.render_chunk().preview(), "person.name");

        father.link(&mut gr1);
        mother.link(&mut gr2);

        assert_eq!(person_name.render_chunk().preview(), "person.name");
        assert_eq!(father_name.render_chunk().preview(), "par.name");
        assert_eq!(mother_name.render_chunk().preview(), "par_2.name");
        assert_eq!(gr1_name.render_chunk().preview(), "person_2.name");
        assert_eq!(gr2_name.render_chunk().preview(), "person_3.name");
    }

    #[test]
    fn test_table_alias() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let mut users = Table::new("users", data_source.clone()).with_column("name");
        let mut roles = Table::new("users", data_source.clone()).with_column("name");

        assert_eq!(
            &users
                .field_query(users.get_column("name").unwrap())
                .preview(),
            "SELECT name FROM users"
        );

        users.alias.enforce_table_in_field_queries();
        assert_eq!(
            &users
                .field_query(users.get_column("name").unwrap())
                .preview(),
            "SELECT users.name FROM users"
        );

        users.link(&mut roles);
        assert_eq!(
            &users
                .field_query(users.get_column("name").unwrap())
                .preview(),
            "SELECT u.name FROM users AS u"
        );
    }
}
