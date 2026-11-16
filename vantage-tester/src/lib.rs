//! What-if scenario tests: the engine. Parses `tests/scenarios/*.yaml`, plans the what-if
//! tree, walks it through a [`ScenarioHost`] and reports green, yellow or red. It knows no
//! backend: Rhai source is carried as text for the host to evaluate.

mod file;
mod host;
mod report;
mod seed;
mod tree;
mod walk;

use std::path::Path;

use vantage_core::{Context, Result, error};

pub use file::{
    ClockOffset, ClockUnit, Do, OneOrMany, RecordSource, Scenario, ScenarioFile, Session, Step,
    Stubs, VarDef, WhatIf,
};
pub use host::{ScenarioHost, StepFailure, StepRun};
pub use report::{FileReport, Outcome, Report, ScenarioReport, Status, StepReport, Write, WriteOp};
pub use seed::seed;
pub use tree::{Branch, Node, PlannedStep, ResolvedStep, Tree, Unwritten, plan};
pub use walk::{Event, walk};

/// Reads and parses `path` under the project `root`. `path` may be relative to `root` or
/// absolute inside it; [`ScenarioFile::path`] keeps it relative to `root` and normalised, since
/// it seeds the scenarios. A path outside `root` is an error.
pub fn load(root: &Path, path: &Path) -> Result<ScenarioFile> {
    let full = root.join(path);
    let text =
        std::fs::read_to_string(&full).context(error!("can't read scenario file", path = full))?;
    let relative = relative_to(root, &full).ok_or_else(|| {
        error!(
            "scenario file is outside the project root",
            path = full,
            root = root
        )
    })?;
    parse(&text, relative)
}

/// `full` relative to `root`, normalised; compared as written, then as canonical paths (a
/// relative root against an absolute path). `None` when it's outside.
fn relative_to(root: &Path, full: &Path) -> Option<String> {
    let relative = match full.strip_prefix(root) {
        Ok(relative) => relative.to_path_buf(),
        Err(_) => {
            let root = root.canonicalize().ok()?;
            full.canonicalize()
                .ok()?
                .strip_prefix(root)
                .ok()?
                .to_path_buf()
        }
    };
    let relative = seed::normalise(&relative.to_string_lossy());
    (relative != ".." && !relative.starts_with("../")).then_some(relative)
}

/// Parses scenario YAML; `path` is the project-relative path the seed hashes. Keys are checked
/// (`deny_unknown_fields`); the rest of validation runs in [`plan`].
pub fn parse(yaml: &str, path: impl Into<String>) -> Result<ScenarioFile> {
    let path = seed::normalise(&path.into());
    let mut file: ScenarioFile = serde_yaml_ng::from_str(yaml)
        .map_err(|e| error!(format!("scenario file: {e}"), path = path))?;
    file.path = path;
    Ok(file)
}
