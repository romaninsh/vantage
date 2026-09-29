//! `!include <path>`: replace the tagged value with the file's text.

use std::path::Path;

use serde_yaml_ng::Value;

/// Resolve every `!include` in `value`. Paths are relative to `dir` and
/// must stay under `root`.
pub(super) fn resolve(value: Value, dir: &Path, root: &Path) -> Result<Value, String> {
    Ok(match value {
        Value::Tagged(tagged) if tagged.tag == "include" => {
            let Value::String(rel) = tagged.value else {
                return Err("!include takes a file path".into());
            };
            Value::String(read(&rel, dir, root)?)
        }
        Value::Tagged(mut tagged) => {
            tagged.value = resolve(tagged.value, dir, root)?;
            Value::Tagged(tagged)
        }
        Value::Mapping(map) => Value::Mapping(
            map.into_iter()
                .map(|(k, v)| Ok((k, resolve(v, dir, root)?)))
                .collect::<Result<_, String>>()?,
        ),
        Value::Sequence(items) => Value::Sequence(
            items
                .into_iter()
                .map(|v| resolve(v, dir, root))
                .collect::<Result<_, String>>()?,
        ),
        other => other,
    })
}

fn read(rel: &str, dir: &Path, root: &Path) -> Result<String, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("scenarios root: {e}"))?;
    let path = dir
        .join(rel)
        .canonicalize()
        .map_err(|e| format!("!include {rel}: {e}"))?;
    if !path.starts_with(&root) {
        return Err(format!("!include {rel}: outside the scenarios directory"));
    }
    std::fs::read_to_string(&path).map_err(|e| format!("!include {rel}: {e}"))
}
