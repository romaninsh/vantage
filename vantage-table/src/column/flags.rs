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
    /// Indexed marks this column as cheap to sort or filter on, hinting to generic UIs that they can offer sort headers and filter inputs without a performance penalty
    Indexed,
    /// Label hints to generic UIs that this column is better shown as a
    /// small status tag attached to the record's title than as its own
    /// column (e.g. a status / state field with a per-value color map)
    Label,
}
