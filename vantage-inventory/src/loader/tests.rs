use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;

use super::{load_yaml_validated, parse_yaml_validated, schema_for_type};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Item {
    name: String,
    #[serde(default)]
    script: Option<String>,
}

#[test]
fn schema_compiles_once_per_type() {
    let a = schema_for_type::<Item>().unwrap();
    let b = schema_for_type::<Item>().unwrap();
    assert!(Arc::ptr_eq(&a, &b));
}

#[test]
fn unknown_key_fails_schema_validation() {
    let err = parse_yaml_validated::<Item>("name: a\nextra: 1\n").unwrap_err();
    let text = format!("{err:#}");
    assert!(text.starts_with("schema validation failed:\n"), "{text}");
    assert!(text.contains("extra"), "{text}");
}

#[test]
fn missing_required_key_fails() {
    assert!(parse_yaml_validated::<Item>("script: x\n").is_err());
}

#[test]
fn included_text_lands_in_a_typed_field() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("item");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("body.rhai"), "1 + 1").unwrap();
    std::fs::write(dir.join("a.yaml"), "name: a\nscript: !include body.rhai\n").unwrap();
    let item: Item = load_yaml_validated(&dir.join("a.yaml"), tmp.path()).unwrap();
    assert_eq!(item.name, "a");
    assert_eq!(item.script.as_deref(), Some("1 + 1"));
}
