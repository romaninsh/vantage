//! Subprocess execution with a locked-down environment.
//!
//! Synchronous on purpose: callers run this inside
//! `tokio::task::spawn_blocking`, so the std blocking call never stalls
//! the async runtime. The child gets a *cleared* environment plus only
//! the declared vars (and optionally `PATH`/`HOME`).

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use vantage_core::{Result, error};

/// Captured result of a single command invocation.
#[derive(Clone, Debug)]
pub struct CmdOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Resolve the program to execute. A `command` that contains a path
/// separator but isn't absolute (e.g. `./scripts/gh-stats.py`) is resolved
/// against `base_dir`; bare names (e.g. `gh`) are left untouched so the OS
/// can find them on `PATH`, and absolute paths pass through.
fn resolve_program(command: &str, base_dir: Option<&Path>) -> PathBuf {
    match base_dir {
        Some(dir) if command.contains('/') && !Path::new(command).is_absolute() => {
            dir.join(command)
        }
        _ => PathBuf::from(command),
    }
}

/// Run `command` with `args`, passing only `env` (plus `PATH`/`HOME` when
/// `pass_path`). When `base_dir` is set it resolves a relative `command`
/// path against it and runs the child with `base_dir` as its working
/// directory. Returns the captured output; a non-zero exit is *not* an
/// error here — the Rhai script decides what to do with `exit_code`.
pub fn run_command(
    command: &str,
    args: &[String],
    env: &IndexMap<String, String>,
    pass_path: bool,
    base_dir: Option<&Path>,
) -> Result<CmdOutput> {
    let mut cmd = std::process::Command::new(resolve_program(command, base_dir));
    cmd.args(args);
    cmd.env_clear();

    if let Some(dir) = base_dir {
        cmd.current_dir(dir);
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_name_is_left_for_path_lookup() {
        // No base dir, and with one: a bare command name is never rewritten.
        assert_eq!(resolve_program("gh", None), PathBuf::from("gh"));
        assert_eq!(
            resolve_program("gh", Some(Path::new("/inv"))),
            PathBuf::from("gh")
        );
    }

    #[test]
    fn relative_path_resolves_against_base_dir() {
        assert_eq!(
            resolve_program("./scripts/gh-stats.py", Some(Path::new("/inv"))),
            PathBuf::from("/inv/./scripts/gh-stats.py")
        );
        assert_eq!(
            resolve_program("scripts/gh-stats.py", Some(Path::new("/inv"))),
            PathBuf::from("/inv/scripts/gh-stats.py")
        );
    }

    #[test]
    fn relative_path_without_base_dir_is_unchanged() {
        assert_eq!(
            resolve_program("./scripts/x.py", None),
            PathBuf::from("./scripts/x.py")
        );
    }

    #[test]
    fn absolute_path_passes_through() {
        assert_eq!(
            resolve_program("/usr/bin/gh", Some(Path::new("/inv"))),
            PathBuf::from("/usr/bin/gh")
        );
    }
}
