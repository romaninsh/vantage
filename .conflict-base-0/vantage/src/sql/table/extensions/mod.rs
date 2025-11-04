//! Table extensions
//!
//! Table extensions are a way to add additional functionality to a table. They
//! are implemented as a trait, and can be added to a table using the
//! [`Table::with_extension()`] method.

use std::sync::Arc;

use anyhow::Result;
pub use soft_delete::SoftDelete;

use crate::sql::Query;

use super::SqlTable;

pub trait TableExtension: std::fmt::Debug + Send + Sync {
    fn init(&self, _table: &mut dyn SqlTable) {}
    fn before_select_query(&self, _table: &dyn SqlTable, _query: &mut Query) -> Result<()> {
        Ok(())
    }
    fn before_delete_query(&self, _table: &dyn SqlTable, _query: &mut Query) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct Hooks {
    hooks: Vec<Arc<Box<dyn TableExtension>>>,
}
impl Hooks {
    pub fn new() -> Self {
        Hooks { hooks: vec![] }
    }
    /// Add new hook to the table
    pub fn add_hook(&mut self, hook: Box<dyn TableExtension>) {
        self.hooks.push(Arc::new(hook));
    }

    pub fn before_select_query(&self, table: &dyn SqlTable, query: &mut Query) -> Result<()> {
        for hook in self.hooks.iter() {
            hook.before_select_query(table, query).unwrap();
        }
        Ok(())
    }
    pub fn before_delete_query(&self, table: &dyn SqlTable, query: &mut Query) -> Result<()> {
        for hook in self.hooks.iter() {
            hook.before_delete_query(table, query).unwrap();
        }
        Ok(())
    }
}

// implement Debug for Hooks
impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks").field("hooks", &self.hooks).finish()
    }
}

impl Clone for Hooks {
    fn clone(&self) -> Self {
        Hooks {
            hooks: self.hooks.clone(),
        }
    }
}

mod soft_delete;
