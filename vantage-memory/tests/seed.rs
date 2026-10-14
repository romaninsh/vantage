//! Loading rows from JSON values and files, and dumping a table back out.

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_memory::{MemoryStore, seed};
use vantage_types::Record;

#[test]
fn load_upserts_rows_with_an_id_and_inserts_rows_without() {
    let t = MemoryStore::new().table("t");
    let n = seed::load(&t, vec![json!({"id": "a", "n": 1}), json!({"n": 2})]).unwrap();
    assert_eq!(n, 2);
    assert_eq!(t.len(), 2);
    assert!(t.get("a").is_some());

    let n = seed::load(&t, vec![json!({"id": "a", "n": 9})]).unwrap();
    assert_eq!(n, 1);
    assert_eq!(t.len(), 2, "a row with an existing id replaces it");
    assert_eq!(
        t.get("a").unwrap().get("n"),
        Some(&CborValue::Integer(9.into()))
    );
}

#[test]
fn load_rejects_a_row_that_is_not_an_object() {
    let t = MemoryStore::new().table("t");
    assert!(seed::load(&t, vec![json!({"id": "a"}), json!(3)]).is_err());
    assert!(seed::load(&t, vec![json!([1, 2])]).is_err());
}

#[test]
fn load_is_quiet_when_the_table_is_quiet() {
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    t.set_quiet(true);
    seed::load(&t, vec![json!({"id": "a"})]).unwrap();
    assert_eq!(t.len(), 1);
    assert!(rx.try_recv().is_err());
}

#[test]
fn dump_returns_rows_in_insertion_order_and_round_trips() {
    let t = MemoryStore::new().table("t");
    let rows = vec![
        json!({"id": "b", "tags": ["x"], "m": {"k": 1.5}}),
        json!({"id": "a", "ok": true}),
    ];
    seed::load(&t, rows.clone()).unwrap();
    assert_eq!(seed::dump(&t).unwrap(), rows);
}

#[test]
fn dump_of_an_empty_table_is_empty() {
    let t = MemoryStore::new().table("t");
    assert!(seed::dump(&t).unwrap().is_empty());
}

#[test]
fn dump_fails_on_a_cell_json_cannot_express() {
    let t = MemoryStore::new().table("t");
    let bytes: Record<CborValue> = [("b".to_string(), CborValue::Bytes(vec![1, 2]))]
        .into_iter()
        .collect();
    t.upsert("a", bytes);
    assert!(seed::dump(&t).is_err());
}

#[test]
fn load_file_reads_yaml_and_json_by_extension() {
    let dir = tempfile::tempdir().unwrap();
    let yaml = dir.path().join("rows.yaml");
    std::fs::write(&yaml, "- { id: a, n: 1 }\n- { id: b, n: 2 }\n").unwrap();
    let yml = dir.path().join("rows.yml");
    std::fs::write(&yml, "- { id: d }\n").unwrap();
    let json_file = dir.path().join("rows.json");
    std::fs::write(&json_file, r#"[{"id": "c"}]"#).unwrap();
    let t = MemoryStore::new().table("t");
    assert_eq!(seed::load_file(&t, &yaml).unwrap(), 2);
    assert_eq!(seed::load_file(&t, &yml).unwrap(), 1);
    assert_eq!(seed::load_file(&t, &json_file).unwrap(), 1);
    assert_eq!(t.ids(), ["a", "b", "d", "c"]);
}

#[test]
fn load_file_reads_any_other_extension_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("rows.data");
    std::fs::write(&data, r#"[{"id": "x"}]"#).unwrap();
    let bare = dir.path().join("rows");
    std::fs::write(&bare, r#"[{"id": "y"}]"#).unwrap();
    let t = MemoryStore::new().table("t");
    seed::load_file(&t, &data).unwrap();
    seed::load_file(&t, &bare).unwrap();
    assert_eq!(t.ids(), ["x", "y"]);
}

#[test]
fn load_file_fails_on_a_missing_or_malformed_file() {
    let dir = tempfile::tempdir().unwrap();
    let t = MemoryStore::new().table("t");
    assert!(seed::load_file(&t, &dir.path().join("absent.yaml")).is_err());
    let bad_json = dir.path().join("bad.json");
    std::fs::write(&bad_json, "[{").unwrap();
    assert!(seed::load_file(&t, &bad_json).is_err());
    let bad_yaml = dir.path().join("bad.yaml");
    std::fs::write(&bad_yaml, "- { id: a\n").unwrap();
    assert!(seed::load_file(&t, &bad_yaml).is_err());
    assert!(t.is_empty());
}
