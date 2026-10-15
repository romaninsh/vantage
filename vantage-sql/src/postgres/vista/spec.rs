// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! YAML-facing types for the PostgreSQL Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresTableBlock {
    /// Override for the PostgreSQL table name. Defaults to the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// A Rhai script that builds the SELECT used as this vista's source,
    /// instead of a physical table. Produces a read-only vista. Requires the
    /// `rhai` feature. Mutually exclusive with `table`.
    ///
    /// When `base` is also set, the script runs in *transform mode*: the base
    /// vista's `select()` is seeded into the engine scope as `base`, so the
    /// script extends an existing query instead of building one from scratch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
    /// Name of another vista to derive this one from. Resolved eagerly at build
    /// time through the factory's spec resolver. The base's columns/relations
    /// are inherited per `inherit`, and the base `select()` is the starting
    /// point for the `rhai` transform. Produces a read-only vista.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// Which of the base vista's columns/relations to inherit. Only meaningful
    /// alongside `base`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherit: Option<InheritBlock>,
    /// Observation-supplied parameters, exposed to the `rhai` script as the
    /// `args` map (`args.month`, `args.lob`, …) so a view can apply
    /// conditions BEFORE aggregation. Set programmatically by the host at
    /// open time — never from YAML (hence no serde).
    #[serde(skip)]
    pub args: Vec<(String, String)>,
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
pub struct PostgresColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postgres: Option<PostgresColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresColumnBlock {
    /// SQL column name when it differs from the spec column name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

pub type PostgresVistaSpec = VistaSpec<PostgresTableExtras, PostgresColumnExtras, NoExtras>;
