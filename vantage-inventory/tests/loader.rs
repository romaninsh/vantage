//! The single-file load pipeline: `!include` expansion, hashing, and
//! schema-validated deserialization.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use vantage_inventory::{
    Document, load_document, load_yaml_validated, parse_document, parse_yaml_validated,
    schema_for_type, validate_into,
};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Item {
    name: String,
    #[serde(default)]
    script: Option<String>,
}

fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

fn load(root: &Path, rel: &str) -> anyhow::Result<Document> {
    load_document(&root.join(rel), root)
}

fn error_of(root: &Path, rel: &str) -> String {
    format!("{:#}", load(root, rel).unwrap_err())
}

fn str_value(s: &str) -> serde_yaml_ng::Value {
    serde_yaml_ng::Value::String(s.into())
}

#[test]
fn a_yaml_include_is_spliced_in_as_data_and_recorded() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "page/shared.yaml",
        "kind: label\nparams: { text: hi }\n",
    );
    write(
        tmp.path(),
        "page/a.yaml",
        "body:\n  - !include shared.yaml\n",
    );

    let doc = load(tmp.path(), "page/a.yaml").unwrap();

    assert_eq!(doc.value["body"][0]["kind"], str_value("label"));
    assert_eq!(doc.value["body"][0]["params"]["text"], str_value("hi"));
    assert_eq!(doc.includes.len(), 1);
    assert!(doc.includes[0].ends_with("page/shared.yaml"));
}

#[test]
fn json_and_yml_includes_are_spliced_as_data() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "page/cols.json",
        r#"[{"name":"id"},{"name":"total"}]"#,
    );
    write(tmp.path(), "page/more.yml", "- name: extra\n");
    write(
        tmp.path(),
        "page/a.yaml",
        "columns: !include cols.json\nmore: !include more.yml\n",
    );

    let doc = load(tmp.path(), "page/a.yaml").unwrap();

    assert_eq!(doc.value["columns"][1]["name"], str_value("total"));
    assert_eq!(doc.value["more"][0]["name"], str_value("extra"));
}

#[test]
fn other_extensions_are_included_as_one_string() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/shaped.rhai", "scenery(\"x\").limit(3)\n");
    write(
        tmp.path(),
        "page/a.yaml",
        "observe:\n  script: !include shaped.rhai\n",
    );

    let doc = load(tmp.path(), "page/a.yaml").unwrap();

    assert_eq!(
        doc.value["observe"]["script"],
        str_value("scenery(\"x\").limit(3)\n")
    );
}

#[test]
fn a_nested_include_resolves_from_the_included_files_directory() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/parts/inner.rhai", "1 + 1");
    write(
        tmp.path(),
        "page/parts/mid.yaml",
        "script: !include inner.rhai\n",
    );
    write(
        tmp.path(),
        "page/a.yaml",
        "observe: !include parts/mid.yaml\n",
    );

    let doc = load(tmp.path(), "page/a.yaml").unwrap();

    assert_eq!(doc.value["observe"]["script"], str_value("1 + 1"));
    assert_eq!(doc.includes.len(), 2);
}

#[test]
fn an_include_may_go_up_a_directory_inside_the_project() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "shared/x.rhai", "x");
    write(tmp.path(), "page/a.yaml", "s: !include ../shared/x.rhai\n");

    let doc = load(tmp.path(), "page/a.yaml").unwrap();

    assert_eq!(doc.value["s"], str_value("x"));
}

#[test]
fn an_include_escaping_the_project_root_is_an_error() {
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("proj");
    write(outer.path(), "secret.txt", "nope");
    write(&root, "page/a.yaml", "s: !include ../../secret.txt\n");

    let err = error_of(&root, "page/a.yaml");

    assert!(err.contains("secret.txt"), "{err}");
    assert!(err.contains("a.yaml"), "{err}");
}

#[test]
fn an_include_cycle_is_an_error_listing_the_chain() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include b.yaml\n");
    write(tmp.path(), "page/b.yaml", "y: !include a.yaml\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("cycle"), "{err}");
    assert!(err.contains("a.yaml") && err.contains("b.yaml"), "{err}");
}

#[test]
fn a_file_including_itself_is_a_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include b.yaml\n");
    write(tmp.path(), "page/b.yaml", "y: !include b.yaml\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("cycle"), "{err}");
}

#[test]
fn an_unknown_tag_is_an_error_naming_the_file_and_the_tag() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !rhai 1 + 1\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("!rhai"), "{err}");
    assert!(err.contains("a.yaml"), "{err}");
}

#[test]
fn an_include_with_a_non_string_operand_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include [a, b]\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("a.yaml"), "{err}");
    assert!(err.contains("sequence"), "{err}");
}

#[test]
fn an_unknown_tag_inside_a_sequence_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "items:\n  - ok\n  - !bogus 1\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("!bogus"), "{err}");
}

#[test]
fn a_missing_include_names_the_including_file_and_the_missing_path() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include gone.rhai\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("a.yaml") && err.contains("gone.rhai"), "{err}");
}

#[test]
fn an_include_that_is_not_utf8_is_an_error_naming_it() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("page/blob.rhai");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, [0xff, 0xfe, 0x00]).unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include blob.rhai\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("blob.rhai"), "{err}");
}

#[test]
fn a_yaml_include_that_does_not_parse_is_an_error_naming_it() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/bad.yaml", "key: [unclosed\n");
    write(tmp.path(), "page/a.yaml", "x: !include bad.yaml\n");

    let err = error_of(tmp.path(), "page/a.yaml");

    assert!(err.contains("bad.yaml"), "{err}");
}

#[test]
fn a_file_included_twice_is_listed_once() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/s.rhai", "s");
    write(
        tmp.path(),
        "page/a.yaml",
        "a: !include s.rhai\nb: !include s.rhai\nc: [!include s.rhai]\n",
    );

    let doc = load(tmp.path(), "page/a.yaml").unwrap();

    assert_eq!(doc.value["a"], doc.value["b"]);
    assert_eq!(doc.value["c"][0], str_value("s"));
    assert_eq!(doc.includes.len(), 1);
}

#[test]
fn the_hash_changes_when_an_included_file_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let s = write(tmp.path(), "page/s.rhai", "one");
    write(tmp.path(), "page/a.yaml", "a: !include s.rhai\n");

    let before = load(tmp.path(), "page/a.yaml").unwrap().hash;
    let same = load(tmp.path(), "page/a.yaml").unwrap().hash;
    std::fs::write(&s, "two").unwrap();
    let after = load(tmp.path(), "page/a.yaml").unwrap().hash;

    assert_eq!(before, same);
    assert_ne!(before, after);
}

#[test]
fn parse_document_resolves_includes_against_the_given_path() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/s.rhai", "from-disk");

    let doc = parse_document(
        "s: !include s.rhai\n",
        &tmp.path().join("page/in-memory.yaml"),
        tmp.path(),
    )
    .unwrap();

    assert_eq!(doc.value["s"], str_value("from-disk"));
}

#[test]
fn text_that_is_not_yaml_is_a_parse_error() {
    let tmp = tempfile::tempdir().unwrap();

    let err = parse_document("key: [unclosed\n", &tmp.path().join("a.yaml"), tmp.path());

    assert!(err.is_err());
}

#[test]
fn loading_a_missing_file_is_an_error_naming_the_path() {
    let tmp = tempfile::tempdir().unwrap();

    let err = error_of(tmp.path(), "page/nope.yaml");

    assert!(err.contains("nope.yaml"), "{err}");
}

#[test]
fn schema_for_type_returns_the_same_compiled_schema_each_time() {
    let a = schema_for_type::<Item>().unwrap();
    let b = schema_for_type::<Item>().unwrap();

    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.json["type"], "object");
}

#[test]
fn an_unknown_key_fails_schema_validation_naming_the_key() {
    let err = parse_yaml_validated::<Item>("name: a\nextra: 1\n").unwrap_err();

    let text = format!("{err:#}");
    assert!(text.contains("schema validation failed"), "{text}");
    assert!(text.contains("extra"), "{text}");
}

#[test]
fn a_missing_required_key_fails_schema_validation() {
    let err = parse_yaml_validated::<Item>("script: x\n").unwrap_err();

    let text = format!("{err:#}");
    assert!(text.contains("name"), "{text}");
}

#[test]
fn a_valid_document_deserializes_into_the_typed_value() {
    let item: Item = parse_yaml_validated("name: a\nscript: run\n").unwrap();

    assert_eq!(item.name, "a");
    assert_eq!(item.script.as_deref(), Some("run"));
}

#[test]
fn validate_into_deserializes_a_loaded_document() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "item/a.yaml", "name: a\n");

    let doc = load(tmp.path(), "item/a.yaml").unwrap();
    let item: Item = validate_into(&doc).unwrap();

    assert_eq!(item.name, "a");
    assert_eq!(item.script, None);
}

#[test]
fn included_text_lands_in_a_typed_field() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "item/body.rhai", "1 + 1");
    let file = write(
        tmp.path(),
        "item/a.yaml",
        "name: a\nscript: !include body.rhai\n",
    );

    let item: Item = load_yaml_validated(&file, tmp.path()).unwrap();

    assert_eq!(item.name, "a");
    assert_eq!(item.script.as_deref(), Some("1 + 1"));
}

#[test]
fn a_validation_failure_from_a_file_names_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let file = write(tmp.path(), "item/a.yaml", "name: a\nextra: 1\n");

    let err = load_yaml_validated::<Item>(&file, tmp.path()).unwrap_err();

    let text = format!("{err:#}");
    assert!(text.contains("a.yaml"), "{text}");
    assert!(text.contains("extra"), "{text}");
}
