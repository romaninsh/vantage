use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub target: String,
    pub kind: ReferenceKind,
    pub foreign_key: String,
}

impl Reference {
    pub fn new(
        name: impl Into<String>,
        target: impl Into<String>,
        kind: ReferenceKind,
        foreign_key: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            target: target.into(),
            kind,
            foreign_key: foreign_key.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceKind {
    HasOne,
    HasMany,
    HasForeign,
}
