use std::collections::HashSet;

use vantage_expressions::{Expression, Expressive, expr_any};

use crate::{column::flags::ColumnFlag, traits::column_like::ColumnLike};

/// Represents a table column with optional alias and flags
#[derive(Debug, Clone)]
pub struct Column {
    name: String,
    alias: Option<String>,
    flags: HashSet<ColumnFlag>,
}

impl Column {
    /// Create a new column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
            flags: HashSet::new(),
        }
    }

    /// Set an alias for this column
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Get the column name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the column alias if set
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    /// Add flags to this column
    pub fn with_flags(mut self, flags: &[ColumnFlag]) -> Self {
        self.flags.extend(flags.iter().cloned());
        self
    }

    /// Add a single flag to this column
    pub fn with_flag(mut self, flag: ColumnFlag) -> Self {
        self.flags.insert(flag);
        self
    }

    /// Get the column flags
    pub fn flags(&self) -> &HashSet<ColumnFlag> {
        &self.flags
    }
}

impl ColumnLike for Column {
    fn name(&self) -> &str {
        &self.name
    }

    fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    fn flags(&self) -> HashSet<ColumnFlag> {
        self.flags.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }

    fn get_type(&self) -> &'static str {
        "column"
    }

    fn clone_box(&self) -> Box<dyn ColumnLike> {
        Box::new(self.clone())
    }
}

impl<T> Expressive<T> for Column {
    fn expr(&self) -> Expression<T> {
        expr_any!(self.name.clone())
    }
}

impl From<&str> for Column {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}
