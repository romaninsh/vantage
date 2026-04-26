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
    /// Indexed marks this column as having a backend-maintained secondary index.
    /// Backends that don't support indexes (CSV, SQL with native indexes) ignore the flag.
    /// vantage-redb uses it to decide which columns get index tables and to gate
    /// which columns can carry conditions.
    Indexed,
}
