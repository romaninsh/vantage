//! Finding a kind's files on disk, and mapping a changed path back to the
//! kind and key it belongs to. Apart from loading documents, every
//! filesystem query the registry makes goes through this module.
//!
//! Keys are file stems. A [`Depth::OneLevel`] folder also reads one level
//! of subfolders and keys those files `<sub>/<stem>`. In folders only
//! `.yaml` files count; READMEs, schemas and files pulled in by
//! `!include` are not leaves.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

use crate::kind::{Depth, Location};

/// Every `(key, path)` a kind at `location` has under `root`, sorted by
/// key. A missing folder or root file yields nothing.
pub(crate) fn files(root: &Path, location: Location) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    match location {
        Location::RootFile(name) => {
            let path = root.join(name);
            if let (true, Some(key)) = (path.exists(), root_key(name)) {
                out.push((key, path));
            }
        }
        Location::Folder { name, depth } => {
            let Ok(entries) = std::fs::read_dir(root.join(name)) else {
                return out;
            };
            for path in entries.flatten().map(|e| e.path()) {
                if path.is_dir() {
                    if depth == Depth::OneLevel {
                        subfolder(&path, &mut out);
                    }
                    continue;
                }
                if let Some(stem) = file_stem(&path) {
                    out.push((stem.to_string(), path));
                }
            }
        }
    }
    out.sort();
    out
}

/// The files of one subfolder, keyed `<sub>/<stem>`. Folders inside it
/// are not read.
fn subfolder(dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Some(sub) = dir.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if let Some(stem) = file_stem(&path) {
            out.push((format!("{sub}/{stem}"), path));
        }
    }
}

fn file_stem(path: &Path) -> Option<&str> {
    yaml_stem(path.file_name()?.to_str()?)
}

fn yaml_stem(name: &str) -> Option<&str> {
    name.strip_suffix(".yaml").filter(|stem| !stem.is_empty())
}

/// Which of `locations` the file at `path` belongs to, and its key.
/// `path` may be spelled through a symlinked root (a watcher on macOS
/// reports `/private/var/...` for a root opened as `/var/...`), and need
/// not exist any more.
pub(crate) fn classify(
    root: &Path,
    locations: impl Iterator<Item = Location>,
    path: &Path,
) -> Option<(usize, String)> {
    let rel = relative(root, path)?;
    let parts = rel
        .components()
        .map(|c| match c {
            Component::Normal(os) => os.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<&str>>>()?;

    locations.enumerate().find_map(|(at, location)| {
        let key = match (location, parts.as_slice()) {
            (Location::RootFile(name), [file]) if name == *file => root_key(name)?,
            (Location::Folder { name, .. }, [folder, file]) if name == *folder => {
                yaml_stem(file)?.to_string()
            }
            (
                Location::Folder {
                    name,
                    depth: Depth::OneLevel,
                },
                [folder, sub, file],
            ) if name == *folder => format!("{sub}/{}", yaml_stem(file)?),
            _ => return None,
        };
        Some((at, key))
    })
}

/// The key of a root file: its name without the extension.
pub(crate) fn root_key(name: &str) -> Option<String> {
    Some(Path::new(name).file_stem()?.to_str()?.to_string())
}

pub(crate) fn exists(path: &Path) -> bool {
    path.exists()
}

pub(crate) fn is_dir(path: &Path) -> bool {
    path.is_dir()
}

/// The canonical spelling of `path`. A deleted file is resolved through
/// its parent folder; `path` itself is returned when neither resolves.
pub(crate) fn canonical_or_same(path: &Path) -> PathBuf {
    path.canonicalize()
        .ok()
        .or_else(|| canonical_via_parent(path))
        .unwrap_or_else(|| path.to_path_buf())
}

/// `path` with its parent folder canonicalized, for a file that need not
/// exist.
fn canonical_via_parent(path: &Path) -> Option<PathBuf> {
    Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?))
}

/// A hash over the raw bytes of `path` (empty when unreadable), for a file
/// too broken to produce a [`Document`](crate::Document) hash.
pub(crate) fn raw_hash(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    std::fs::read(path).unwrap_or_default().hash(&mut hasher);
    hasher.finish()
}

/// `path` relative to `root`, trying the spelling given, then the
/// canonical root, then the canonical parent of `path`.
fn relative(root: &Path, path: &Path) -> Option<PathBuf> {
    if let Ok(rel) = path.strip_prefix(root) {
        return Some(rel.to_path_buf());
    }
    let root = root.canonicalize().ok()?;
    if let Ok(rel) = path.strip_prefix(&root) {
        return Some(rel.to_path_buf());
    }
    let path = canonical_via_parent(path)?;
    Some(path.strip_prefix(&root).ok()?.to_path_buf())
}
