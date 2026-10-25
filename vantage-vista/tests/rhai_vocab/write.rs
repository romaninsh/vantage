use std::sync::Arc;

use serde_json::json;
use vantage_core::error;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Column, ColumnFlag, IdGenerator, ShortId, Vista, VistaMetadata, VistaResolver,
};

use super::support::{eval, host_with, json, read_write, read_write_terminals, run, store};

#[test]
fn an_insert_without_an_id_mints_from_the_table_generator() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("short").insert(#{ a: 1 })"#);
    assert_eq!(id.chars().count(), 10, "{id}");
    assert!(id.chars().all(|c| ShortId::BASE58.contains(c)), "{id}");
    assert_eq!(
        json(&host, &format!(r#"table("short").get("{id}")"#))["a"],
        json!(1)
    );
}

#[test]
fn a_failing_generator_fails_the_insert() {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_id_column("id")
        .with_id_generator(IdGenerator::fallible(|| Err(error!("no ids today"))));
    let shell = MockShell::new().with_metadata(metadata);
    let resolver: Arc<dyn VistaResolver> =
        Arc::new(move |name: &str| Ok(Vista::new(name, shell.clone())));
    let host = host_with(resolver, read_write_terminals());
    let err = run(&host, r#"table("t").insert(#{ a: 1 })"#).unwrap_err();
    assert!(err.contains("no ids today"), "{err}");
}

#[test]
fn an_explicit_id_wins_over_the_table_generator() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("short").insert(#{ id: "mine", a: 1 })"#);
    assert_eq!(id, "mine");
}

#[test]
fn an_insert_of_an_existing_id_returns_it_and_writes_nothing() {
    let host = read_write(&store());
    let id: String = eval(&host, r#"table("short").insert(#{ id: "s1", a: 1 })"#);
    let again: String = eval(&host, r#"table("short").insert(#{ id: "s1", a: 2 })"#);
    assert_eq!(again, id);
    assert_eq!(json(&host, r#"table("short").get("s1")"#)["a"], json!(1));
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
