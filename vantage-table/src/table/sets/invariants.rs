//! Apply a table's set invariants to a record before it is written.
//!
//! A `Table` narrowed by a literal `column = value` (an id scope or a traversed
//! relation) carries that pair as an *invariant* (see `Table::invariants`).
//! Full-record writes conform (fill absent/null columns, reject a different
//! value); patches validate (never fill, reject a null or different value).
//! Both the typed-entity and the raw-record write paths funnel through here.

use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::{InvariantValue, Record};

/// Conform `record` to `invariants`: fill absent/null columns, keep equal
/// values, `Conflict` on a different one.
pub(crate) fn enforce_invariants<V: InvariantValue>(
    record: &mut Record<V>,
    invariants: &IndexMap<String, V>,
) -> Result<()> {
    vantage_dataset::invariants::conform(record, invariants)
}

/// Check a patch against `invariants` without adding anything.
pub(crate) fn validate_invariants<V: InvariantValue>(
    partial: &Record<V>,
    invariants: &IndexMap<String, V>,
) -> Result<()> {
    vantage_dataset::invariants::validate(partial, invariants)
}
