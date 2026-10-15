// @llm-protected: public API must not be extended on a whim. All features are implemented, do not add duplicates.
//! YAML-facing types for the MySQL Vista driver.

use serde::{Deserialize, Serialize};
use vantage_vista::VistaSpec;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mysql: Option<MysqlTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlTableBlock {
    /// Override for the MySQL table name. Defaults to the spec name.
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
    /// A Rhai script applied to the built vista as its final build step, with
    /// the vista exposed as `self`, e.g.
    /// `self.with_condition(sql::ident("is_paying_client") == true)`. Composes with
    /// `table`/`rhai`/`base` (it runs last). Requires the `rhai` feature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modify: Option<String>,
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
pub struct MysqlColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mysql: Option<MysqlColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlColumnBlock {
    /// SQL column name when it differs from the spec column name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlReferenceExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mysql: Option<MysqlRefBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MysqlRefBlock {
    /// Rhai script that builds the traversal target for this reference, instead
    /// of the default foreign-key eq-condition path. Evaluated lazily at
    /// traversal time with the parent row in scope as `row` and `me`. Requires
    /// the `rhai` feature; ignored by builds without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rhai: Option<String>,
}

pub type MysqlVistaSpec = VistaSpec<MysqlTableExtras, MysqlColumnExtras, MysqlReferenceExtras>;
