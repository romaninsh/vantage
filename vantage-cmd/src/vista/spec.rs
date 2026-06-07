//! YAML-facing types for the command Vista driver.
//!
//! A vista's `cmd:` block carries the Rhai script and optional command /
//! env overrides. The command and shared env usually come from the
//! pre-built [`Cmd`](crate::Cmd) the factory is bound to, so most vistas
//! only need `cmd.rhai`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CmdTableExtras {
    pub cmd: CmdBlock,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CmdBlock {
    /// Per-table command override. Defaults to the factory's `Cmd` command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Per-table env, merged over (and winning against) the datasource env.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub env: IndexMap<String, String>,
    /// The Rhai script: builds the argv, calls `run(args)`, parses output.
    pub rhai: String,
    /// Optional per-row detail script (opt into two-pass loading). Runs the
    /// same locked command with `id` in scope to hydrate one record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CmdColumnExtras {}

pub type CmdVistaSpec = VistaSpec<CmdTableExtras, CmdColumnExtras, NoExtras>;
