//! # SurrealDB Thing (Record ID)
//!
//! doc wip

use vantage_expressions::{OwnedExpression, expr};

/// SurrealDB Thing (record ID) representation
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::thing::Thing;
///
/// // doc wip
/// let thing = Thing::new("users".to_string(), "john".to_string());
/// let parsed = Thing::from_str("users:john");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Thing {
    table: String,
    id: String,
}

impl Thing {
    /// Creates a new Thing with table and ID
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `table` - doc wip
    /// * `id` - doc wip
    pub fn new(table: String, id: String) -> Self {
        Self { table, id }
    }

    /// Parses a Thing from string format "table:id"
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `thing_str` - doc wip
    ///
    /// # Returns
    ///
    /// doc wip
    pub fn from_str(thing_str: &str) -> Option<Self> {
        if let Some((table, id)) = thing_str.split_once(':') {
            Some(Self {
                table: table.to_string(),
                id: id.to_string(),
            })
        } else {
            None
        }
    }
}

impl Into<OwnedExpression> for Thing {
    fn into(self) -> OwnedExpression {
        expr!(format!("{}:{}", self.table, self.id))
    }
}
