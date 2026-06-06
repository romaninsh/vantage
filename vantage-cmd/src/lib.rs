//! `vantage-cmd` — a Vantage persistence backend that gets its data by
//! **running a local command** (the `aws` CLI, `kubectl`, `gh`, …).
//!
//! The design pins down a strict security boundary and hands everything
//! else to a [Rhai](https://rhai.rs) script:
//!
//! - The **command is locked** on the [`Cmd`] datasource — a script can
//!   never change which binary runs.
//! - **Environment variables are locked** — the child process gets a
//!   *cleared* environment plus only the vars declared on the datasource
//!   / table (and `PATH`/`HOME` so the binary is locatable, toggleable
//!   via [`Cmd::with_pass_path`]).
//! - The **arguments and the output parsing are scripted in Rhai**. When
//!   a table is read, the script runs with the table's `conditions`,
//!   `columns`, `limit`, `offset` and `id_column` in scope; it builds an
//!   argv, calls the registered `run(args)` callback (which executes the
//!   locked command), then parses the captured output into rows.
//!
//! ```no_run
//! # use vantage_cmd::Cmd;
//! # use vantage_table::table::Table;
//! # use vantage_types::EmptyEntity;
//! # async fn run() -> vantage_core::Result<()> {
//! const LOG_GROUPS: &str = r#"
//!     let args = ["logs", "describe-log-groups", "--output", "json"];
//!     for c in conditions {
//!         if c.field == "logGroupNamePrefix" {
//!             args += ["--log-group-name-prefix", c.value];
//!         }
//!     }
//!     let out = run(args);
//!     if out.exit_code != 0 { throw out.stderr; }
//!     parse_json(out.stdout).logGroups
//! "#;
//!
//! let cmd = Cmd::new("aws")
//!     .with_env("AWS_REGION", "us-east-1")
//!     .with_script("log.groups", LOG_GROUPS);
//!
//! let groups = Table::<Cmd, EmptyEntity>::new("log.groups", cmd)
//!     .with_id_column("logGroupName")
//!     .with_column_of::<i64>("creationTime");
//! # let _ = groups;
//! # Ok(()) }
//! ```

mod cmd;
mod condition;
mod exec;
mod expr_data_source;
mod operation;
mod rhai_engine;
mod table_source;
mod types;

pub mod models;
pub mod vista;

pub use cmd::{Cmd, CmdSpec};
pub use condition::{CmdCondition, eq};
pub use exec::CmdOutput;
pub use operation::CmdOperation;
pub use types::AnyCmdType;
