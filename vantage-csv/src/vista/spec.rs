//! YAML-facing types for the CSV Vista driver.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvTableExtras {
    pub csv: CsvBlock,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvBlock {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csv: Option<CsvColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvColumnBlock {
    /// CSV header to read this column from when it differs from the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub type CsvVistaSpec = VistaSpec<CsvTableExtras, CsvColumnExtras, NoExtras>;
