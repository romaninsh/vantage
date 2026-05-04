//! Canonical column flag vocabulary.
//!
//! Vista carries flags on columns as plain strings — the set is open so
//! drivers and consumers can extend it. The constants below name the flags
//! understood directly by `vantage-vista`'s own accessors. Drivers translate
//! their native flag types into these strings when constructing a `Vista`.

pub const ID: &str = "id";
pub const TITLE: &str = "title";
pub const SEARCHABLE: &str = "searchable";
pub const MANDATORY: &str = "mandatory";
pub const HIDDEN: &str = "hidden";
