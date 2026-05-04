use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogWriterTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_writer: Option<LogWriterBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogWriterBlock {
    /// Override the file stem; defaults to `spec.name`. The full path is
    /// `{base_dir}/{filename}.jsonl`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

pub type LogWriterVistaSpec = VistaSpec<LogWriterTableExtras, NoExtras, NoExtras>;
