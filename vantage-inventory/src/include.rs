//! The `!include <path>` pass: runs on the parsed `serde_yaml_ng::Value`
//! before it becomes JSON, because `Value::Tagged` has no JSON form. When
//! this pass is done no tag is left behind — `!include` is spliced, any
//! other tag is a hard error naming the file and the tag.
//!
//! Semantics:
//! - the path is relative to the directory of the file that contains the
//!   tag (nested includes resolve against the included file's own directory);
//! - after canonicalising, the target must sit under the project root —
//!   `../` inside the project is fine, escaping it is an error;
//! - `.yaml` / `.yml` / `.json` are parsed, recursed and spliced as data;
//!   anything else is slurped as one string (a `.rhai` script, a `.md`
//!   description, a `.sql` query);
//! - a file included twice is read once per load; an include cycle is an
//!   error listing the chain;
//! - every resolved path is recorded so a watcher can reload the including
//!   document when a dependency changes.
//!
//! YAML merge keys (`<<:`) are not applied by this loader — they never were —
//! so `!include` cannot be combined with them.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_yaml_ng::Value;

const INCLUDE_TAG: &str = "!include";

/// One parsed document: the tag-free YAML value, every file it pulled in
/// through `!include`, and a hash over the file bytes plus every include —
/// so an edit to a dependency changes the hash of the document that uses it.
#[derive(Debug, Clone)]
pub struct Document {
    pub value: Value,
    pub includes: Vec<PathBuf>,
    pub hash: u64,
}

pub(crate) struct Includer {
    root: PathBuf,
    cache: HashMap<PathBuf, Value>,
    includes: Vec<PathBuf>,
    /// Canonical paths currently being expanded, outermost first.
    stack: Vec<PathBuf>,
    hasher: DefaultHasher,
}

impl Includer {
    pub(crate) fn new(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("project root {} is unreadable", root.display()))?;
        Ok(Self {
            root,
            cache: HashMap::new(),
            includes: Vec::new(),
            stack: Vec::new(),
            hasher: DefaultHasher::new(),
        })
    }

    /// Fold `bytes` into the composite hash. Called for the main document
    /// and once per include, in resolution order.
    pub(crate) fn hash_bytes(&mut self, bytes: &[u8]) {
        bytes.hash(&mut self.hasher);
    }

    pub(crate) fn finish(self, value: Value) -> Document {
        Document {
            value,
            includes: self.includes,
            hash: self.hasher.finish(),
        }
    }

    /// Expand every `!include` under `value`. `file` is the document the
    /// value came from (relative includes resolve against its directory);
    /// it need not exist on disk for an in-memory document.
    pub(crate) fn expand(&mut self, value: Value, file: &Path) -> Result<Value> {
        let base_dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        self.expand_in(value, &base_dir, file)
    }

    fn expand_in(&mut self, value: Value, base_dir: &Path, file: &Path) -> Result<Value> {
        match value {
            Value::Tagged(tagged) => {
                if tagged.tag != INCLUDE_TAG {
                    bail!(
                        "{}: unknown tag `{}` — only `{INCLUDE_TAG}` is supported",
                        file.display(),
                        tagged.tag
                    );
                }
                let Value::String(rel) = tagged.value else {
                    bail!(
                        "{}: `{INCLUDE_TAG}` takes a path string, got {}",
                        file.display(),
                        describe(&tagged.value)
                    );
                };
                self.include(&rel, base_dir, file)
            }
            Value::Sequence(items) => items
                .into_iter()
                .map(|v| self.expand_in(v, base_dir, file))
                .collect::<Result<Vec<_>>>()
                .map(Value::Sequence),
            Value::Mapping(map) => {
                let mut out = serde_yaml_ng::Mapping::with_capacity(map.len());
                for (k, v) in map {
                    let k = self.expand_in(k, base_dir, file)?;
                    let v = self.expand_in(v, base_dir, file)?;
                    out.insert(k, v);
                }
                Ok(Value::Mapping(out))
            }
            scalar => Ok(scalar),
        }
    }

    fn include(&mut self, rel: &str, base_dir: &Path, from: &Path) -> Result<Value> {
        let joined = base_dir.join(rel);
        let path = joined.canonicalize().with_context(|| {
            format!(
                "{}: include `{rel}` not found (looked at {})",
                from.display(),
                joined.display()
            )
        })?;
        if !path.starts_with(&self.root) {
            bail!(
                "{}: include `{rel}` escapes the project ({} is outside {})",
                from.display(),
                path.display(),
                self.root.display()
            );
        }
        if let Some(at) = self.stack.iter().position(|p| p == &path) {
            let chain: Vec<String> = self.stack[at..]
                .iter()
                .chain(std::iter::once(&path))
                .map(|p| p.display().to_string())
                .collect();
            bail!("{}: include cycle: {}", from.display(), chain.join(" -> "));
        }
        if let Some(cached) = self.cache.get(&path) {
            return Ok(cached.clone());
        }

        let bytes = std::fs::read(&path).with_context(|| {
            format!("{}: cannot read include {}", from.display(), path.display())
        })?;
        self.hash_bytes(&bytes);
        self.includes.push(path.clone());

        let value = if is_data_file(&path) {
            let text = String::from_utf8(bytes).with_context(|| {
                format!(
                    "{}: include {} is not UTF-8",
                    from.display(),
                    path.display()
                )
            })?;
            let raw: Value = serde_yaml_ng::from_str(&text)
                .with_context(|| format!("parsing include {}", path.display()))?;
            self.stack.push(path.clone());
            let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
            let expanded = self.expand_in(raw, &base, &path);
            self.stack.pop();
            expanded?
        } else {
            Value::String(String::from_utf8(bytes).with_context(|| {
                format!(
                    "{}: include {} is not UTF-8",
                    from.display(),
                    path.display()
                )
            })?)
        };
        self.cache.insert(path, value.clone());
        Ok(value)
    }
}

fn is_data_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml" | "yml" | "json")
    )
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a bool",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Sequence(_) => "a sequence",
        Value::Mapping(_) => "a mapping",
        Value::Tagged(_) => "a tagged value",
    }
}

#[cfg(test)]
mod tests;
