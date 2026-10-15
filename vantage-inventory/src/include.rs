//! The `!include <path>` pass, run on the parsed YAML before it becomes JSON
//! (`Value::Tagged` has no JSON form). Afterwards no tag is left: `!include`
//! is spliced, any other tag is an error naming the file and the tag.
//!
//! - The path is relative to the including file's directory and must stay
//!   under the project root.
//! - `.yaml` / `.yml` / `.json` are spliced as data; anything else is one string.
//! - A file included twice is read once; a cycle is an error listing the chain.
//! - Every resolved path is recorded so a watcher can reload dependents.
//!
//! YAML merge keys (`<<:`) are not applied, so `!include` cannot be combined
//! with them.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_yaml_ng::Value;

const INCLUDE_TAG: &str = "!include";

/// A parsed, tag-free document, the files it pulled in through `!include`,
/// and a hash over its bytes plus every include's.
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
    /// Canonical paths being expanded, outermost first; a repeat is a cycle.
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

    /// `file` need not exist on disk; only its directory is used.
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
