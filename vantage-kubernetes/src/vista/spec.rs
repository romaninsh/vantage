//! YAML-facing types for the Kubernetes Vista driver.
//!
//! Placeholders for v1 — tables are built from the typed constructors in
//! [`crate::models`]. When the YAML inventory app lands, the natural extras
//! to surface are a per-table `api_path` and per-column `json_path` /
//! projector hints; sketch them then.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubeTableExtras {}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KubeColumnExtras {}

pub type KubeVistaSpec = VistaSpec<KubeTableExtras, KubeColumnExtras, NoExtras>;
