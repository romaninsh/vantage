use serde::{Deserialize, Serialize};

/// Display-relevant column metadata held by a `Vista`.
///
/// Vista deliberately does not carry `vantage_table::ColumnFlag`. Driver
/// factories translate flags into vista accessors (id column, title columns,
/// hidden flag) when constructing the Vista.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub original_type: String,
    pub hidden: bool,
}

impl Column {
    pub fn new(name: impl Into<String>, original_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            original_type: original_type.into(),
            hidden: false,
        }
    }

    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }
}
