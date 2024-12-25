use crate::{sql::Operations, uniqid::UniqueIdVendor};
use std::sync::{Arc, Mutex, RwLock};

/// TableAlias is a shareable configuration for table alias configuration,
/// accessible by columns of said table.
///
/// Columns can request table alias configuration at any time through
/// table_alias.try_get() (equal to read().unwrap().try_get()) which returns
/// optional table alias. This will inform column if it should prefix
/// itself with a table.
///
/// Column may want to explicitly use a table alias through table_alias.get(),
/// which will return either table alias or table name.
///
/// If Table makes use of joins, the aliases will be automatically assigned
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
    custom_alias: Option<String>,

    // Column ambiguity - alwaps prefix all columns with table
    enforce_table_alias: bool,

    // Shared table alias generator, for linked tables
    alias_vendor: Arc<Mutex<UniqueIdVendor>>,
}

impl TableAliasConfig {
    pub fn new(table_name: &str) -> Self {
        TableAliasConfig {
            table_name: table_name.to_string(),
            custom_alias: None,
            enforce_table_alias: false,
            alias_vendor: Arc::new(Mutex::new(UniqueIdVendor::new())),
        }
    }

    /// When our table joins other tables, we enforce table alias. This can also be
    /// used explicitly by the user to resolve conflicts.
    pub fn set_enforce(&mut self) {
        if self.enforce_table_alias {
            return;
        }
        if !self.custom_alias.is_some() {
            // we will treat table name as alias now, so we must reserve it
            let t = self.table_name.clone();
            self.set(&t);
        }
        self.enforce_table_alias = true;
    }

    /// Check if table/alias prefixing should be enforced?
    pub fn get_enforce(&self) -> bool {
        self.enforce_table_alias
    }

    /// Use custom alias for this table. If alias was used previously, it won't be reserved
    /// anymore.
    pub fn set(&mut self, alias: &str) {
        if self.custom_alias.is_some() {
            let old = self.custom_alias.clone().unwrap();
            self.alias_vendor.lock().unwrap().dont_avoid(&old);
        }
        let alias = self.alias_vendor.lock().unwrap().get_uniq_id(alias);
        self.custom_alias = Some(alias);
    }

    /// Used by a column if it wants to be explicitly prefixed (e.g. used in subquery)
    pub fn get(&self) -> String {
        if self.custom_alias.is_some() {
            self.custom_alias.clone().unwrap()
        } else {
            self.table_name.clone()
        }
    }

    /// Used by a column natively, to guard against situations when we join more tables
    /// and therefore all fields should be prefixed to avoid ambiguity
    pub fn try_get(&self) -> Option<String> {
        if self.enforce_table_alias {
            Some(self.get())
        } else {
            None
        }
    }

    pub fn deep_clone(&self) -> Self {
        TableAliasConfig {
            table_name: self.table_name.clone(),
            custom_alias: self.custom_alias.clone(),
            enforce_table_alias: self.enforce_table_alias,
            alias_vendor: Arc::new(Mutex::new(UniqueIdVendor::new())),
        }
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
    pub fn set_enforce(&self) {
        self.config.write().unwrap().set_enforce();
    }
    pub fn get_enforce(&self) -> bool {
        self.config.read().unwrap().get_enforce()
    }
    pub fn try_get(&self) -> Option<String> {
        self.config.read().unwrap().try_get()
    }
    pub fn get(&self) -> String {
        self.config.read().unwrap().get()
    }
    pub fn set(&self, alias: &str) {
        self.config.write().unwrap().set(alias);
    }
    pub fn deep_clone(&self) -> Self {
        Self {
            config: Arc::new(RwLock::new(self.config.read().unwrap().deep_clone())),
        }
    }

    pub fn merge(&mut self, other: &mut Self) {
        if Arc::ptr_eq(&self.config, &other.config) {
            panic!("Merging with self not allowed");
        }
        self.set_enforce();
        other.set_enforce();
        let s_w = self.config.write().unwrap();
        let mut o_w = other.config.write().unwrap();

        if Arc::ptr_eq(&s_w.alias_vendor, &o_w.alias_vendor) {
            panic!("ID Vendor is identical");
        }

        let mut s_mg = s_w.alias_vendor.lock().unwrap();
        let mut o_mg = o_w.alias_vendor.lock().unwrap();

        s_mg.merge(&mut *o_mg);
        drop(o_mg);

        o_w.alias_vendor = s_w.alias_vendor.clone();
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        expr_arc,
        prelude::{ExpressionArc, PgValueColumn},
        sql::Chunk,
    };
    use serde_json::json;

    use crate::{mocks::MockDataSource, prelude::AnyTable, sql::Table};

    #[test]
    fn test_table_cloning() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let table = Table::new("users", data_source.clone()).with_column("name");

        let table2 = table.clone();

        assert_eq!(table.alias.get_enforce(), false);
        assert_eq!(table2.alias.get_enforce(), false);

        table.alias.set_enforce();

        assert_eq!(table.alias.get_enforce(), true);
        assert_eq!(table2.alias.get_enforce(), false);
    }

    #[test]
    fn test_aliasconfig_merging() {
        let data = json!([]);
        let data_source = MockDataSource::new(&data);
        let table = Table::new("users", data_source.clone()).with_column("name");

        let mut table2 = table.clone();

        table2
            .alias
            .config
            .write()
            .unwrap()
            .alias_vendor
            .lock()
            .unwrap()
            .avoid("table1");

        let mut x = table.alias;

        x.merge(&mut table2.alias);

        let r = x.config.read().unwrap();
        let mut v = r.alias_vendor.lock().unwrap();

        // After merging, "table1" will be avoided
        assert_eq!(v.get_uniq_id("table1"), "table1_2");

        // users cannot be used either, because it was set by table2. However we are not
        assert_eq!(v.get_uniq_id("users"), "users_2");
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
        table.alias.set_enforce();
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
}
