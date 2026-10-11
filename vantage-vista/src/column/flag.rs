//! [`ColumnFlag`]: the column flag vocabulary.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A flag on a [`Column`](crate::Column).
///
/// The eight named flags are the ones the data layer acts on. Any other name
/// is kept as [`Other`](Self::Other) for the layer that interprets it (a UI's
/// presentation flag such as `label`); vista never reads those, and the layer
/// that owns the YAML schema rejects names it doesn't know.
///
/// In YAML and serde a flag is its snake_case name. `ColumnFlag::from("label")`
/// and deserializing map the eight known names onto their variants. Flags
/// compare and hash by name, so an `Other` built by hand from a known name
/// (`Other("id")`) is the same flag as the variant (`Id`).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ColumnFlag {
    /// The column is the row's id.
    Id,
    /// The column names the row for a reader.
    Title,
    /// Quicksearch reads the column.
    Searchable,
    /// The backend can order by the column.
    Orderable,
    /// A write must carry a value for the column.
    Mandatory,
    /// Readers don't show the column by default.
    Hidden,
    /// The column is computed, not stored: an implicit-reference traversal
    /// (`country.name`), a server-side expression, or a computed column of
    /// the Vista. Readers render it read-only and writes drop it.
    Calculated,
    /// The backend makes the column's values (auto-increment, generated record
    /// ids). On the id column, an insert without an id asks the backend for one.
    Auto,
    /// A flag interpreted by a layer above vista.
    Other(String),
}

impl ColumnFlag {
    /// The flag's snake_case name.
    pub fn as_str(&self) -> &str {
        match self {
            ColumnFlag::Id => "id",
            ColumnFlag::Title => "title",
            ColumnFlag::Searchable => "searchable",
            ColumnFlag::Orderable => "orderable",
            ColumnFlag::Mandatory => "mandatory",
            ColumnFlag::Hidden => "hidden",
            ColumnFlag::Calculated => "calculated",
            ColumnFlag::Auto => "auto",
            ColumnFlag::Other(name) => name,
        }
    }
}

impl PartialEq for ColumnFlag {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ColumnFlag {}

impl std::hash::Hash for ColumnFlag {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl From<&str> for ColumnFlag {
    fn from(name: &str) -> Self {
        match name {
            "id" => ColumnFlag::Id,
            "title" => ColumnFlag::Title,
            "searchable" => ColumnFlag::Searchable,
            "orderable" => ColumnFlag::Orderable,
            "mandatory" => ColumnFlag::Mandatory,
            "hidden" => ColumnFlag::Hidden,
            "calculated" => ColumnFlag::Calculated,
            "auto" => ColumnFlag::Auto,
            other => ColumnFlag::Other(other.to_string()),
        }
    }
}

impl From<String> for ColumnFlag {
    fn from(name: String) -> Self {
        ColumnFlag::from(name.as_str())
    }
}

impl fmt::Display for ColumnFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ColumnFlag {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ColumnFlag {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(ColumnFlag::from(String::deserialize(deserializer)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_names_map_onto_variants() {
        let flags: Vec<ColumnFlag> = serde_yaml_ng::from_str("[id, searchable, label]").unwrap();
        assert_eq!(
            flags,
            [
                ColumnFlag::Id,
                ColumnFlag::Searchable,
                ColumnFlag::Other("label".into())
            ]
        );
        assert_eq!(
            serde_yaml_ng::to_string(&flags).unwrap(),
            "- id\n- searchable\n- label\n"
        );
    }

    #[test]
    fn a_hand_built_other_with_a_known_name_is_that_flag() {
        assert_eq!(ColumnFlag::Other("id".into()), ColumnFlag::Id);
        let column = crate::Column::new("a", "int").with_flag(ColumnFlag::Other("hidden".into()));
        assert!(column.has_flag(ColumnFlag::Hidden));
        assert!(matches!(ColumnFlag::from("hidden"), ColumnFlag::Hidden));
    }
}
