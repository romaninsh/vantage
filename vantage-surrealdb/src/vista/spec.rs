//! YAML-facing types for the SurrealDB Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::VistaSpec;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal: Option<SurrealTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealTableBlock {
    /// Override for the SurrealDB table name. Defaults to the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// A Rhai script that builds the SELECT this vista reads, instead of a
    /// physical table. Produces a read-only vista. Requires the `rhai`
    /// feature. Mutually exclusive with `table`.
    ///
    /// With `base` set, the script transforms the base vista's `select()`,
    /// which it sees as `base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
    /// Another vista to derive this one from, resolved at build time through
    /// the factory's spec resolver. `inherit` picks its columns and relations;
    /// its `select()` is the starting point for `rhai`. Produces a read-only
    /// vista.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Which of the base vista's columns and relations to inherit. Only
    /// meaningful alongside `base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit: Option<InheritBlock>,
    /// A Rhai script run last, on the built vista exposed as `self`. It
    /// narrows it with the usual verbs plus native conditions YAML can't
    /// express, e.g. `self.with_condition(surreal::ident("is_paying_client") == true)`.
    /// Composes with `table`, `rhai` and `base`. Requires the `rhai` feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify: Option<String>,
    /// Host-set arguments, seen by the script as the `args` map
    /// (`args.bakery`, …), so a view can filter before aggregating. Never set
    /// from YAML. They reach `rhai:` sources and `base:` transforms; a
    /// `modify:` script runs without them.
    #[serde(skip)]
    pub args: Vec<(String, String)>,
}

/// The `args` a driver block carries (`""` means not set).
pub trait DriverBlockArgs {
    fn driver_block_args(&self) -> Vec<(String, String)>;
}

impl<C, R> DriverBlockArgs for VistaSpec<SurrealTableExtras, C, R> {
    fn driver_block_args(&self) -> Vec<(String, String)> {
        self.driver
            .surreal
            .as_ref()
            .map(|b| b.args.clone())
            .unwrap_or_default()
    }
}

/// Selects which parts of a `base` vista a derived vista inherits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InheritBlock {
    /// Base column names to copy onto the derived table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// Base relation names to copy onto the derived table.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal: Option<SurrealColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealColumnBlock {
    /// SurrealDB field name when it differs from the spec column name.
    /// The vista surfaces values under the spec name; the underlying field
    /// is read/written under this alias.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// Per-reference SurrealDB block, nested under `surreal:` so it can't collide
/// with a reference's `table`/`kind`/`foreign_key` keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealReferenceExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal: Option<SurrealRefBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealRefBlock {
    /// A Rhai script that builds this reference's traversal target in place
    /// of the foreign-key match, run at traversal time with the parent `row`
    /// in scope. Requires the `rhai` feature; a build without it fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
}

pub type SurrealVistaSpec =
    VistaSpec<SurrealTableExtras, SurrealColumnExtras, SurrealReferenceExtras>;
