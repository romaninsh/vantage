//! Files the inventory writes into a project for editors and for git:
//! versioned JSON schemas, folder READMEs, and a managed `.gitignore` block.
//! A write is skipped when the file already holds the wanted text.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::Inventory;
use crate::kind::Location;

/// First line of the managed `.gitignore` block.
pub const GITIGNORE_BEGIN: &str =
    "# --- managed by Vantage: generated files (do not edit this block)";
/// Last line of the managed `.gitignore` block.
pub const GITIGNORE_END: &str = "# --- end managed by Vantage";

/// The files one [`Inventory::emit_schemas`] call changed, in the order it
/// changed them. Files that already held the wanted text are not listed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EmitReport {
    /// READMEs and schemas written.
    pub written: Vec<PathBuf>,
    /// Older schema versions deleted.
    pub removed: Vec<PathBuf>,
}

impl Inventory {
    /// Create every folder kind's folder, write its README and its schema
    /// as `<folder>-schema-<version>.json`, and delete older
    /// `<folder>-schema-<N>.json` files that no `*.yaml` in the folder names
    /// in a `# yaml-language-server: $schema=./<file>` header (first ten
    /// lines). Bump `version` when a kind's value changes shape. Root-file
    /// kinds get nothing. Stops at the first error, including a kind whose
    /// [`schema`](crate::Kind::schema) fails or a YAML file that can't be
    /// read; nothing is deleted then.
    pub fn emit_schemas(&self, version: u32) -> Result<EmitReport> {
        let mut report = EmitReport::default();
        for kind in self.kinds() {
            let Location::Folder { name, .. } = kind.location() else {
                continue;
            };
            let folder = self.root().join(name);
            std::fs::create_dir_all(&folder)
                .with_context(|| format!("create folder {}", folder.display()))?;
            if let Some(readme) = kind.readme() {
                write_if_changed(&folder.join("README.md"), readme, &mut report)?;
            }
            let schema = kind
                .schema()
                .with_context(|| format!("schema for kind `{}`", kind.id()))?;
            let schema = serde_json::to_string_pretty(&schema).context("serialize schema")?;
            let current = schema_file_name(name, version);
            write_if_changed(&folder.join(&current), &schema, &mut report)?;
            remove_unreferenced_schemas(&folder, name, &current, &mut report)?;
        }
        Ok(report)
    }
}

/// `<folder>-schema-<version>.json`.
pub fn schema_file_name(folder: &str, version: u32) -> String {
    format!("{folder}-schema-{version}.json")
}

/// Keep the managed block in `root/.gitignore` equal to `lines`. The block
/// replaces an existing one in place; a file without both markers gets it
/// appended after a blank line. Returns whether the file was written. A
/// file that exists but cannot be read as text is an error and is left
/// as it is.
pub fn ensure_gitignore(root: &Path, lines: &[&str]) -> Result<bool> {
    let path = root.join(".gitignore");
    let existing = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| format!("read {}", path.display())),
    };

    let mut block = format!("{GITIGNORE_BEGIN}\n");
    for line in lines {
        block.push_str(line);
        block.push('\n');
    }
    block.push_str(GITIGNORE_END);

    let want = match (existing.find(GITIGNORE_BEGIN), existing.find(GITIGNORE_END)) {
        (Some(start), Some(end)) if end > start => {
            let end = end + GITIGNORE_END.len();
            format!("{}{}{}", &existing[..start], block, &existing[end..])
        }
        // No markers, or a half-deleted pair: append rather than guess.
        _ if existing.trim().is_empty() => format!("{block}\n"),
        _ => format!("{}\n\n{}\n", existing.trim_end(), block),
    };
    if existing == want {
        return Ok(false);
    }
    std::fs::write(&path, &want).with_context(|| format!("write {}", path.display()))?;
    Ok(true)
}

fn write_if_changed(path: &Path, want: &str, report: &mut EmitReport) -> Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|existing| existing == want) {
        return Ok(());
    }
    std::fs::write(path, want).with_context(|| format!("write {}", path.display()))?;
    report.written.push(path.to_path_buf());
    Ok(())
}

fn remove_unreferenced_schemas(
    folder: &Path,
    name: &str,
    current: &str,
    report: &mut EmitReport,
) -> Result<()> {
    let referenced = schema_references(folder)?;
    let mut stale: Vec<PathBuf> = std::fs::read_dir(folder)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|file| {
                    is_schema_version(name, file) && file != current && !referenced.contains(file)
                })
        })
        .collect();
    stale.sort();
    for path in stale {
        std::fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        report.removed.push(path);
    }
    Ok(())
}

/// Whether `file` is `<name>-schema-<N>.json` with an integer `N`.
fn is_schema_version(name: &str, file: &str) -> bool {
    file.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix("-schema-"))
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|n| n.parse::<u32>().is_ok())
}

/// A `*.yaml` that can't be read is an error: its reference is unknown.
fn schema_references(folder: &Path) -> Result<HashSet<String>> {
    let mut refs = HashSet::new();
    for entry in std::fs::read_dir(folder)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let content =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        for line in content.lines().take(10) {
            let Some(rest) = line.strip_prefix("# yaml-language-server:") else {
                continue;
            };
            if let Some(target) = rest.split('=').nth(1) {
                let file = target.trim().trim_start_matches("./").trim();
                if !file.is_empty() {
                    refs.insert(file.to_string());
                }
            }
        }
    }
    Ok(refs)
}
