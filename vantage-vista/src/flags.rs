//! Canonical column flag vocabulary.
//!
//! Vista carries flags on columns as plain strings — the set is open so
//! drivers and consumers can extend it. The constants below name the flags
//! understood directly by `vantage-vista`'s own accessors. Drivers translate
//! their native flag types into these strings when constructing a `Vista`.

pub const ID: &str = "id";
pub const TITLE: &str = "title";
pub const SEARCHABLE: &str = "searchable";
pub const ORDERABLE: &str = "orderable";
pub const MANDATORY: &str = "mandatory";
pub const HIDDEN: &str = "hidden";
/// Read-only computed column: an implicit-reference traversal
/// (`country.name`), an `expr:` script, or a lazy computed column — flagged by
/// driver factories via `Table::is_calculated_column`. Consumers should render
/// it read-only and exclude it from forms and write payloads; the data layer
/// enforces the same on writes (imported columns are stripped or rejected).
pub const CALCULATED: &str = "calculated";
