//! Subprocess execution with a locked-down environment.
//!
//! Synchronous on purpose: callers run this inside
//! `tokio::task::spawn_blocking`, so the std blocking call never stalls
//! the async runtime. The child gets a *cleared* environment plus only
//! the declared vars (and optionally `PATH`/`HOME`).

use indexmap::IndexMap;
use vantage_core::{Result, error};

/// Captured result of a single command invocation.
#[derive(Clone, Debug)]
pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Run `command` with `args`, passing only `env` (plus `PATH`/`HOME` when
/// `pass_path`). Returns the captured output; a non-zero exit is *not* an
/// error here — the Rhai script decides what to do with `exit_code`.
pub fn run_command(
    command: &str,
    args: &[String],
    env: &IndexMap<String, String>,
    pass_path: bool,
) -> Result<CmdOutput> {
    let mut cmd = std::process::Command::new(command);
    cmd.args(args);
    cmd.env_clear();

    if pass_path {
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
        if let Ok(home) = std::env::var("HOME") {
            cmd.env("HOME", home);
        }
    }
    for (k, v) in env {
        cmd.env(k, v);
    }

    let output = cmd.output().map_err(|e| {
        error!(
            "failed to execute command",
            command = command.to_string(),
            detail = e.to_string()
        )
    })?;

    Ok(CmdOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}
