use serde::{Deserialize, Serialize};

use crate::flags;

/// Display-relevant column metadata held by a `Vista`.
///
/// `flags` is an open vocabulary; the constants in [`crate::flags`] name the
/// values vista's own accessors understand. Drivers and consumers may add
/// their own.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub original_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
}

impl Column {
    pub fn new(name: impl Into<String>, original_type: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            original_type: original_type.into(),
            flags: Vec::new(),
        }
    }

    pub fn with_flag(mut self, flag: impl Into<String>) -> Self {
        self.flags.push(flag.into());
        self
    }

    pub fn hidden(self) -> Self {
        self.with_flag(flags::HIDDEN)
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    pub fn is_hidden(&self) -> bool {
        self.has_flag(flags::HIDDEN)
    }

    pub fn is_id(&self) -> bool {
        self.has_flag(flags::ID)
    }

    pub fn is_title(&self) -> bool {
        self.has_flag(flags::TITLE)
    }
}
