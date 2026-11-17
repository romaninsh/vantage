//! The load pipeline: read → parse → `!include` → hash → JSON →
//! JSON-schema validate → deserialize. Schemas compile once per Rust type
//! and are cached for the process.

use std::any::TypeId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, RwLock};

use anyhow::{Context, Result, anyhow};
use jsonschema::error::ValidationErrorKind;
use jsonschema::{JSONSchema, ValidationError};
use schemars::{JsonSchema, schema_for};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::include::{Document, Includer};

/// A type's schema, both as the JSON an editor consumes and compiled for
/// validation by [`validate_into`].
pub struct CachedSchema {
    pub json: Value,
    compiled: JSONSchema,
}

// The cache hands one `Arc` to every thread, so this must stay Send + Sync.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<CachedSchema>();
};

static SCHEMAS: LazyLock<RwLock<HashMap<TypeId, Arc<CachedSchema>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// The cached schema for `T`, built on first use.
pub fn schema_for_type<T: JsonSchema + 'static>() -> Result<Arc<CachedSchema>> {
    let id = TypeId::of::<T>();
    if let Some(s) = SCHEMAS.read().unwrap().get(&id) {
        return Ok(s.clone());
    }
    let json = serde_json::to_value(schema_for!(T)).context("serialize schema")?;
    let compiled =
        JSONSchema::compile(&json).map_err(|e| anyhow!("failed to compile schema: {e}"))?;
    // Two threads can race to build the same schema; the first insert wins.
    let entry = Arc::new(CachedSchema { json, compiled });
    Ok(SCHEMAS.write().unwrap().entry(id).or_insert(entry).clone())
}

/// Read, parse and resolve includes for the file at `path`, confined to
/// `root`. The result is tag-free and carries its composite hash.
pub fn load_document(path: &Path, root: &Path) -> Result<Document> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read inventory file {}", path.display()))?;
    let text = String::from_utf8(bytes)
        .with_context(|| format!("inventory file {} is not UTF-8", path.display()))?;
    parse_document(&text, path, root).with_context(|| format!("in {}", path.display()))
}

/// [`load_document`] over text already in memory. `path` is where the text
/// came from — includes resolve relative to its directory.
pub fn parse_document(text: &str, path: &Path, root: &Path) -> Result<Document> {
    let raw: serde_yaml_ng::Value = serde_yaml_ng::from_str(text).context("yaml parsing error")?;
    let mut inc = Includer::new(root)?;
    inc.hash_bytes(text.as_bytes());
    let value = inc.expand(raw, path)?;
    Ok(inc.finish(value))
}

/// Schema-validate a parsed document and deserialize it. Unknown kinds and
/// unknown keys fail here — before anything is built.
pub fn validate_into<T>(doc: &Document) -> Result<T>
where
    T: JsonSchema + DeserializeOwned + 'static,
{
    let json: Value = serde_json::to_value(&doc.value).context("yaml→json conversion error")?;
    let schema = schema_for_type::<T>()?;
    if let Err(errors) = schema.compiled.validate(&json) {
        let messages: Vec<String> = errors
            .flat_map(|err| {
                let mut lines = vec![format!("{}: {}", err.instance_path, err)];
                if matches!(err.kind, ValidationErrorKind::OneOfNotValid) {
                    lines.extend(branch_errors(&schema.json, &err));
                }
                lines
            })
            .collect();
        return Err(anyhow!(
            "schema validation failed:\n{}",
            messages.join("\n")
        ));
    }
    serde_json::from_value(json).context("deserialize error")
}

/// A `oneOf` failure says only that no branch matched. When the instance
/// names its branch with a string `kind`, that branch's own errors (an
/// unknown key, a missing one) say what is wrong; empty otherwise.
fn branch_errors(root: &Value, err: &ValidationError<'_>) -> Vec<String> {
    let Some(kind) = err.instance.get("kind").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(Value::Array(branches)) = walk(root, &err.schema_path.clone().into_vec()) else {
        return Vec::new();
    };
    let Some(branch) = branches.iter().map(|b| resolve(root, b)).find(|b| {
        let tag = &b["properties"]["kind"];
        tag["const"] == kind
            || tag["enum"]
                .as_array()
                .is_some_and(|e| e.iter().any(|v| v == kind))
    }) else {
        return Vec::new();
    };
    let mut standalone = branch.clone();
    if let (Some(obj), Some(defs)) = (standalone.as_object_mut(), root.get("definitions")) {
        obj.insert("definitions".into(), defs.clone());
    }
    let Ok(compiled) = JSONSchema::compile(&standalone) else {
        return Vec::new();
    };
    let Err(errors) = compiled.validate(&err.instance) else {
        return Vec::new();
    };
    errors
        .map(|e| {
            format!(
                "{}{}: {e} (kind `{kind}`)",
                err.instance_path, e.instance_path
            )
        })
        .collect()
}

/// The schema node at `path` (a validation error's schema path, which
/// passes through `$ref`s without naming them).
fn walk<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut node = root;
    for segment in path {
        let step = |n: &'a Value| match n {
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => n.get(segment.as_str()),
        };
        node = match step(node) {
            Some(next) => next,
            None => step(resolve(root, node))?,
        };
    }
    Some(node)
}

/// `node`, or the definition its `$ref` points at.
fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    node.get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix('#'))
        .and_then(|pointer| root.pointer(pointer))
        .unwrap_or(node)
}

/// [`load_document`] then [`validate_into`].
pub fn load_yaml_validated<T>(path: &Path, root: &Path) -> Result<T>
where
    T: JsonSchema + DeserializeOwned + 'static,
{
    let doc = load_document(path, root)?;
    validate_into(&doc).with_context(|| format!("in {}", path.display()))
}

/// Parse and validate an in-memory document with no file behind it. An
/// `!include` in it resolves against the current directory: for tests and
/// fixtures, not project files.
pub fn parse_yaml_validated<T>(content: &str) -> Result<T>
where
    T: JsonSchema + DeserializeOwned + 'static,
{
    let doc = parse_document(content, Path::new("inline.yaml"), Path::new("."))?;
    validate_into(&doc)
}
