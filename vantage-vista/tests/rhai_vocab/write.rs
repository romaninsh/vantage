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
fn insert_existing_id_is_a_no_op() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("t").insert(#{ id: "r1", a: 5 })"#);
    assert_eq!(id, "r1");
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
fn insert_without_id_mints_a_uuid_v7() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("t").insert(#{ a: 7 })"#);
    let parsed = uuid::Uuid::parse_str(&id).expect("a UUID");
    assert_eq!(parsed.get_version_num(), 7);
}

#[test]
fn server_made_ids_ask_the_backend() {
    let host = read_write(&store());
    assert_eq!(
        eval::<String>(&host, r#"table("auto").insert(#{ a: 1 })"#),
        "1"
    );
}

#[test]
fn numeric_id_without_auto_needs_an_id() {
    let host = read_write(&store());
    let err = run(&host, r#"table("intid").insert(#{ a: 1 })"#).unwrap_err();
    assert!(err.contains("declare server-made ids"), "{err}");
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
fn delete_missing_returns_true() {
    let host = read_write(&store());
    assert!(eval::<bool>(&host, r#"table("t").delete("nope")"#));
}

#[test]
fn writes_stay_in_the_set() {
    let host = read_write(&store());
    assert!(eval::<bool>(
        &host,
        r#"table("t").where("a", 1).delete("r2")"#
    ));
    assert_eq!(
        json(&host, r#"table("t").ids()"#),
        json!(["r1", "r2", "r3"])
    );
}

#[test]
fn row_by_row_import_skips_existing_ids() {
    let host = read_write(&store());
    assert_eq!(
        json(&host, r#"table("mock").import_from(table("t"))"#),
        json!({"inserted": 2, "skipped": 1, "rejected": 0, "cancelled": false})
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
        json!({"inserted": 2, "skipped": 0, "rejected": 0, "cancelled": false})
    );
    assert_eq!(
        json(&host, r#"table("copy").list()"#),
        json!([{"id": "c-r1", "a": 3, "tag": "x"}, {"id": "c-r3", "a": 2, "tag": "x"}])
    );
}

#[test]
fn import_counts_rows_outside_the_set_as_rejected() {
    let host = read_write(&store());
    assert_eq!(
        json(
            &host,
            r#"table("copy").where("tag", "x").import_from(table("t"), #{ id: "c-${row.id}", tag: "${row.a}" })"#
        ),
        json!({"inserted": 0, "skipped": 0, "rejected": 3, "cancelled": false})
    );
    assert_eq!(
        json(
            &host,
            r#"table("copy").where("tag", "x").import_from(table("t"), #{ id: "c-${row.id}", n: "${row.n}" })"#
        ),
        json!({"inserted": 3, "skipped": 0, "rejected": 0, "cancelled": false})
    );
}
