use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde_yaml_ng::Value;

use super::{Document, Includer};

fn write(dir: &Path, rel: &str, text: &str) -> PathBuf {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, text).unwrap();
    p
}

/// Expand `root/rel` with the include pass alone, flattening the error
/// chain into one message so tests can match on any part of it.
fn load(root: &Path, rel: &str) -> Result<Document> {
    let path = root.join(rel);
    let text = fs::read_to_string(&path).unwrap();
    let raw: Value = serde_yaml_ng::from_str(&text).context("yaml parsing error")?;
    let mut inc = Includer::new(root)?;
    inc.hash_bytes(text.as_bytes());
    let value = inc.expand(raw, &path).map_err(|e| anyhow!("{e:#}"))?;
    Ok(inc.finish(value))
}

#[test]
fn yaml_include_is_spliced_as_data() {
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
    let body = &doc.value["body"];
    assert_eq!(body[0]["kind"], Value::String("label".into()));
    assert_eq!(doc.includes.len(), 1);
    assert!(doc.includes[0].ends_with("page/shared.yaml"));
}

#[test]
fn other_extensions_are_slurped_as_strings() {
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
        Value::String("scenery(\"x\").limit(3)\n".into())
    );
}

#[test]
fn nested_include_resolves_from_the_included_files_dir() {
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
    assert_eq!(
        doc.value["observe"]["script"],
        Value::String("1 + 1".into())
    );
    assert_eq!(doc.includes.len(), 2);
}

#[test]
fn dotdot_inside_the_root_is_fine() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "shared/x.rhai", "x");
    write(tmp.path(), "page/a.yaml", "s: !include ../shared/x.rhai\n");
    let doc = load(tmp.path(), "page/a.yaml").unwrap();
    assert_eq!(doc.value["s"], Value::String("x".into()));
}

#[test]
fn escaping_the_root_is_an_error() {
    let outer = tempfile::tempdir().unwrap();
    let root = outer.path().join("proj");
    write(outer.path(), "secret.txt", "nope");
    write(&root, "page/a.yaml", "s: !include ../../secret.txt\n");
    let err = load(&root, "page/a.yaml").unwrap_err().to_string();
    assert!(err.contains("escapes the project"), "{err}");
}

#[test]
fn cycle_is_an_error_listing_the_chain() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include b.yaml\n");
    write(tmp.path(), "page/b.yaml", "y: !include a.yaml\n");
    let err = load(tmp.path(), "page/a.yaml").unwrap_err().to_string();
    assert!(err.contains("include cycle"), "{err}");
    assert!(
        err.contains("b.yaml -> ") && err.contains("a.yaml"),
        "{err}"
    );
}

#[test]
fn self_include_is_a_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include b.yaml\n");
    write(tmp.path(), "page/b.yaml", "y: !include b.yaml\n");
    let err = load(tmp.path(), "page/a.yaml").unwrap_err().to_string();
    assert!(err.contains("include cycle"), "{err}");
}

#[test]
fn unknown_tag_is_an_error_naming_file_and_tag() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !rhai 1 + 1\n");
    let err = load(tmp.path(), "page/a.yaml").unwrap_err().to_string();
    assert!(err.contains("unknown tag `!rhai`"), "{err}");
    assert!(err.contains("a.yaml"), "{err}");
}

#[test]
fn missing_include_names_both_paths() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include gone.rhai\n");
    let err = format!("{:#}", load(tmp.path(), "page/a.yaml").unwrap_err());
    assert!(err.contains("a.yaml") && err.contains("gone.rhai"), "{err}");
}

#[test]
fn a_file_included_twice_is_read_once_and_listed_once() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/s.rhai", "s");
    write(
        tmp.path(),
        "page/a.yaml",
        "a: !include s.rhai\nb: !include s.rhai\nc: [!include s.rhai]\n",
    );
    let doc = load(tmp.path(), "page/a.yaml").unwrap();
    assert_eq!(doc.value["a"], doc.value["b"]);
    assert_eq!(doc.value["c"][0], Value::String("s".into()));
    assert_eq!(doc.includes.len(), 1);
}

#[test]
fn hash_changes_when_an_include_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let s = write(tmp.path(), "page/s.rhai", "one");
    write(tmp.path(), "page/a.yaml", "a: !include s.rhai\n");
    let before = load(tmp.path(), "page/a.yaml").unwrap().hash;
    let same = load(tmp.path(), "page/a.yaml").unwrap().hash;
    assert_eq!(before, same);
    fs::write(&s, "two").unwrap();
    let after = load(tmp.path(), "page/a.yaml").unwrap().hash;
    assert_ne!(before, after);
}

#[test]
fn json_include_is_data() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "page/cols.json",
        r#"[{"name":"id"},{"name":"total"}]"#,
    );
    write(tmp.path(), "page/a.yaml", "columns: !include cols.json\n");
    let doc = load(tmp.path(), "page/a.yaml").unwrap();
    assert_eq!(
        doc.value["columns"][1]["name"],
        Value::String("total".into())
    );
}

#[test]
fn include_with_a_non_string_operand_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "page/a.yaml", "x: !include [a, b]\n");
    let err = load(tmp.path(), "page/a.yaml").unwrap_err().to_string();
    assert!(err.contains("takes a path string"), "{err}");
}
