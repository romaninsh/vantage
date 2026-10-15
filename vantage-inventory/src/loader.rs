//! The load pipeline: read → parse → `!include` → hash → JSON →
//! JSON-schema validate → deserialize. Schemas compile once per Rust type
//! and are cached for the process.

use std::any::TypeId;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, RwLock};

use anyhow::{Context, Result, anyhow};
use jsonschema::JSONSchema;
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
            .map(|err| format!("{}: {}", err.instance_path, err))
            .collect();
        return Err(anyhow!(
            "schema validation failed:\n{}",
            messages.join("\n")
        ));
    }
    serde_json::from_value(json).context("deserialize error")
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
