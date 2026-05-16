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

/// Cardinality of a relation. Cross-persistence-ness is no longer
/// encoded here — it's determined at resolution time by whether the
/// target Vista lives in the same driver or a different one (the
/// inventory loader knows). YAML specs that previously used
/// `kind: has_foreign` migrate to `kind: has_one` or `kind: has_many`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    #[default]
    HasOne,
    HasMany,
}
