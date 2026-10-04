use serde_json::json;

use super::support::{eval, json, read_write, run, store};

#[test]
fn insert_with_explicit_id_keeps_it() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("t").insert(#{ id: "r9", a: 5 })"#);
    assert_eq!(id, "r9");
    assert_eq!(json(&host, r#"table("t").get("r9")"#)["a"], json!(5));
}

#[test]
fn insert_duplicate_id_errors() {
    let host = read_write(&store());
    assert!(run(&host, r#"table("t").insert(#{ id: "r1", a: 5 })"#).is_err());
    assert_eq!(json(&host, r#"table("t").get("r1")"#)["a"], json!(1));
}

#[test]
fn insert_without_id_returns_generated() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("t").insert(#{ a: 7 })"#);
    let script = format!(r#"table("t").get("{id}")"#);
    assert_eq!(json(&host, &script)["a"], json!(7));
}

#[test]
fn upsert_inserts_then_replaces() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("t").upsert("r9", #{ a: 1 })"#);
    assert_eq!(id, "r9");
    assert_eq!(json(&host, r#"table("t").get("r9")"#)["a"], json!(1));
    eval::<String>(&host, r#"table("t").upsert("r9", #{ a: 2 })"#);
    assert_eq!(json(&host, r#"table("t").get("r9")"#)["a"], json!(2));
}

#[test]
fn patch_missing_returns_false() {
    let host = read_write(&store());
    assert!(!eval::<bool>(
        &host,
        r#"table("t").patch("nope", #{ a: 9 })"#
    ));
    assert!(eval::<bool>(&host, r#"table("t").patch("r1", #{ a: 9 })"#));
    assert_eq!(
        json(&host, r#"table("t").get("r1")"#),
        json!({"id": "r1", "a": 9, "n": 3})
    );
}

#[test]
fn delete_missing_returns_false() {
    let host = read_write(&store());
    assert!(!eval::<bool>(&host, r#"table("t").delete("nope")"#));
}

#[test]
fn writes_ignore_narrowing() {
    let host = read_write(&store());
    assert!(eval::<bool>(
        &host,
        r#"table("t").where("a", 1).delete("r2")"#
    ));
    assert_eq!(json(&host, r#"table("t").ids()"#), json!(["r1", "r3"]));
}

#[test]
fn row_by_row_import_skips_existing_ids() {
    let host = read_write(&store());
    assert_eq!(
        json(&host, r#"table("mock").import_from(table("t"))"#),
        json!({"inserted": 2, "skipped": 1, "cancelled": false})
    );
    assert_eq!(json(&host, r#"table("mock").get("r1")"#)["a"], json!(7));
    assert_eq!(json(&host, r#"table("mock").get("r3")"#)["n"], json!(2));
}

#[test]
fn import_copies_rows_through_mapping() {
    let host = read_write(&store());
    assert_eq!(
        json(
            &host,
            r#"table("copy").import_from(table("t").where("a", 1), #{ id: "c-${row.id}", a: "${row.n}", tag: "x" })"#,
        ),
        json!({"inserted": 2, "skipped": 0, "cancelled": false})
    );
    assert_eq!(
        json(&host, r#"table("copy").list()"#),
        json!([{"id": "c-r1", "a": 3, "tag": "x"}, {"id": "c-r3", "a": 2, "tag": "x"}])
    );
}
