/// Column flags that define behavior and properties of columns.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ColumnFlag {
    /// Mandatory will require read/write operations to always have value for this field, it cannot be missing
    Mandatory,
    /// Hidden columns are excluded from UI display. Display only: it says
    /// nothing about whether rows carry the column.
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
    /// The column is a request parameter: its value is sent to the backend
    /// (a URL placeholder, an API argument), and returned rows needn't carry
    /// it. [`Table::row_fields`](crate::table::Table::row_fields) leaves it
    /// out.
    RequestParam,
}

impl ColumnFlag {
    /// The Vista flag this flag becomes in a table's Vista metadata. Hints
    /// for generic UIs (`Indexed`, `Label`) stay [`Other`] flags: they grant
    /// no capability, so `Indexed` is not `Orderable`.
    ///
    /// [`Other`]: vantage_vista::ColumnFlag::Other
    pub fn to_vista(&self) -> vantage_vista::ColumnFlag {
        use vantage_vista::ColumnFlag as Vista;
        match self {
            ColumnFlag::Mandatory => Vista::Mandatory,
            ColumnFlag::Hidden => Vista::Hidden,
            ColumnFlag::IdField => Vista::Id,
            ColumnFlag::TitleField => Vista::Title,
            ColumnFlag::Searchable => Vista::Searchable,
            ColumnFlag::Indexed => Vista::Other("indexed".into()),
            ColumnFlag::Label => Vista::Other("label".into()),
            ColumnFlag::RequestParam => Vista::RequestParam,
        }
    }
}
