use serde_json::Value;
use std::collections::HashSet;
use std::marker::PhantomData;

use vantage_expressions::{Expression, Expressive, expr_any};

use crate::{column::flags::ColumnFlag, traits::column_like::ColumnLike};

pub trait ColumnType: Sync + Send + std::fmt::Debug + Clone + 'static {}

// Blanket implementation for any type that satisfies the requirements
impl<T> ColumnType for T where T: Sync + Send + std::fmt::Debug + Clone + 'static {}

/// Represents a table column with optional alias and flags
#[derive(Debug, Clone)]
pub struct Column<T = Value>
where
    T: ColumnType,
{
    name: String,
    alias: Option<String>,
    flags: HashSet<ColumnFlag>,
    _phantom: PhantomData<T>,
}

impl<T> Column<T>
where
    T: ColumnType,
{
    /// Create a new column with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            alias: None,
            flags: HashSet::new(),
            _phantom: PhantomData,
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

impl<T> ColumnLike<T> for Column<T>
where
    T: ColumnType,
{
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

    // get_type() uses the trait default implementation: std::any::type_name::<T>()
}

impl<T> Expressive<T> for Column<T>
where
    T: ColumnType,
{
    fn expr(&self) -> Expression<T> {
        expr_any!(self.name.clone())
    }
}

impl From<&str> for Column<Value> {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}
