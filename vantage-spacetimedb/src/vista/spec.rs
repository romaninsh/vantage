//! YAML-facing types for the SpacetimeDB driver.
//!
//! There is very little to declare, and that is the point. A SpacetimeDB module
//! publishes its own schema — columns, types, primary keys, unique constraints,
//! whether a relation is a table or a view — so a spec does not have to restate
//! any of it. The module is the authority; the YAML only names which relation to
//! surface and, optionally, narrows what it shows.
//!
//! The one extra worth having is [`SpacetimeTableExtras::relation`], for when a
//! vista should be called something other than the table behind it.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

/// Table-level YAML block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpacetimeTableExtras {
    /// The table or view in the module, when it differs from the vista's name.
    ///
    /// Defaults to the vista name, which is the common case — `name: account`
    /// needs nothing further.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
}

/// Per-column YAML block. Nothing yet: a column's type and flags come from the
/// module definition, and inventing driver-specific overrides before anyone
/// needs them would be inventing a second source of truth.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpacetimeColumnExtras {}

/// A SpacetimeDB vista spec. References use [`NoExtras`] because a module
/// declares no foreign keys — see `capabilities_for`.
pub type SpacetimeVistaSpec = VistaSpec<SpacetimeTableExtras, SpacetimeColumnExtras, NoExtras>;
