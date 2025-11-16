/// Column flags that define behavior and properties of columns.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ColumnFlag {
    /// Mandatory will require read/write operations to always have value for this field, it cannot be missing
    Mandatory,
    /// Hidden columns should be excluded from UI display
    Hidden,
    /// IdField marks this column as the primary identifier for the table
    IdField,
    /// TitleField marks this column as the display title/name for records
    TitleField,
    /// Searchable marks this column as searchable in text searches
    Searchable,
}
