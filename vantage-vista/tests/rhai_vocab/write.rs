use serde_json::json;

use super::support::{eval, json, read_write, run, store};

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

/// A narrowed target imports row by row.
#[test]
fn row_by_row_import_skips_existing_ids() {
    let host = read_write(&store());
    eval::<String>(
        &host,
        r#"table("copy").insert(#{ id: "r1", a: 1, tag: "kept" })"#,
    );
    assert_eq!(
        json(
            &host,
            r#"table("copy").where("a", 1).import_from(table("t").where("a", 1))"#
        ),
        json!({"inserted": 1, "skipped": 1, "rejected": 0, "cancelled": false})
    );
    assert_eq!(
        json(&host, r#"table("copy").get("r1")"#)["tag"],
        json!("kept")
    );
    assert_eq!(json(&host, r#"table("copy").get("r3")"#)["n"], json!(2));
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
