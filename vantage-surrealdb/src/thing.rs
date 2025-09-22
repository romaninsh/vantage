//! # SurrealDB Thing (Record ID)
//!
//! doc wip

use crate::operation::Expressive;
use std::str::FromStr;
use vantage_expressions::{Expression, expr, protocol::expressive::IntoExpressive};

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
/// let parsed = "users:john".parse::<Thing>();
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
    pub fn new(table: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            id: id.into(),
        }
    }
}

impl FromStr for Thing {
    type Err = String;

    fn from_str(thing_str: &str) -> Result<Self, Self::Err> {
        if let Some((table, id)) = thing_str.split_once(':') {
            Ok(Self {
                table: table.to_string(),
                id: id.to_string(),
            })
        } else {
            Err(format!("Invalid thing format: {}", thing_str))
        }
    }
}

impl Expressive for Thing {
    fn expr(&self) -> Expression {
        expr!(format!("{}:{}", self.table, self.id))
    }
}

impl From<Thing> for Expression {
    fn from(val: Thing) -> Self {
        val.expr()
    }
}

impl From<Thing> for IntoExpressive<Expression> {
    fn from(thing: Thing) -> Self {
        IntoExpressive::nested(thing.expr())
    }
}
