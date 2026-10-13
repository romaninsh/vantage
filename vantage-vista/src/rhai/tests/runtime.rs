use std::sync::Arc;

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_types::Record;

use crate::mocks::MockShell;
use crate::rhai::write::new_id;
use crate::rhai::{Writes, preview_script, run_script};
use crate::vista::Vista;
use crate::{Column, ColumnFlag, JoinKey, Reference, VistaMetadata, VistaResolver};

fn user(id: &str, name: &str) -> Record<CborValue> {
    [("id", id), ("name", name)]
        .into_iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.into())))
        .collect()
}

/// Three users, with a `posts` has-many joined on `author`.
fn users_vista() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(
            Column::new("name", "String")
                .with_flag(ColumnFlag::Title)
                .with_flag(ColumnFlag::Orderable),
        )
        .with_column(Column::untyped("nick"))
        .with_id_column("id")
        .with_reference(Reference::has_many(
            "posts",
            "posts",
            JoinKey::new("id", "author"),
        ));
    let source = MockShell::new()
        .with_record("1", user("1", "Alice"))
        .with_record("2", user("2", "Bob"))
        .with_record("3", user("3", "Carol"))
        .with_metadata(metadata)
        .with_ref_target("posts", MockShell::new());
    Vista::new("users", source)
}

fn resolver() -> Arc<dyn VistaResolver> {
    Arc::new(|name: &str| {
        if name == "users" {
            Ok(users_vista())
        } else {
            Err(vantage_core::error!("unknown table", table = name))
        }
    })
}

async fn run(script: &str, limit: usize) -> Result<serde_json::Value, String> {
    run_script(script.into(), resolver(), limit, Writes::Allowed).await
}

#[tokio::test(flavor = "multi_thread")]
async fn list_caps_rows_at_limit() {
    let json = run(r#"table("users").list()"#, 2).await.unwrap();
    let rows = json.as_array().expect("array");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["name"], json!("Alice"));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_clamps_to_max() {
    let json = run(r#"table("users").list()"#, 9999).await.unwrap();
    assert_eq!(json.as_array().unwrap().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn where_then_first_narrows() {
    let json = run(r#"table("users").where("id", "3").first()"#, 5)
        .await
        .unwrap();
    assert_eq!(json["name"], json!("Carol"));
}

#[tokio::test(flavor = "multi_thread")]
async fn capabilities_is_a_flag_map() {
    let json = run(r#"table("users").capabilities()"#, 5).await.unwrap();
    assert_eq!(json["window"], json!(true));
    assert_eq!(json["cursor"], json!(false));
}

#[tokio::test(flavor = "multi_thread")]
async fn columns_lists_schema() {
    let json = run(r#"table("users").columns()"#, 5).await.unwrap();
    let cols = json.as_array().unwrap();
    assert!(cols.iter().any(|c| c["name"] == json!("name")));
    assert_eq!(cols[1]["type"], json!("String"));
    assert_eq!(cols[2]["name"], json!("nick"));
    assert_eq!(
        cols[2]["type"],
        json!(null),
        "an untyped column's type is ()"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn references_name_their_kind_as_yaml_does() {
    let json = run(r#"table("users").references()"#, 5).await.unwrap();
    assert_eq!(
        json,
        json!([{"name": "posts", "kind": "has_many", "contained": false}])
    );
}

#[test]
fn an_untyped_id_column_takes_a_client_made_id() {
    let untyped = Vista::new(
        "notes",
        MockShell::new().with_metadata(
            VistaMetadata::new()
                .with_column(Column::untyped("id"))
                .with_id_column("id"),
        ),
    );
    assert!(new_id(&untyped, "id").is_ok());

    let numeric = Vista::new(
        "notes",
        MockShell::new().with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "int"))
                .with_id_column("id"),
        ),
    );
    let err = new_id(&numeric, "id").unwrap_err();
    assert!(err.to_string().contains("id column holds numbers"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_table_is_an_error() {
    let err = run(r#"table("ghosts").list()"#, 5).await.unwrap_err();
    assert!(err.contains("unknown table"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn syntax_error_is_reported() {
    assert!(!run("this is not rhai (", 5).await.unwrap_err().is_empty());
}

#[test]
fn preview_renders_the_built_query() {
    let script = r#"table("users").where("id", "3").sort("name", "desc")"#;
    let json = preview_script(script.into(), resolver()).unwrap();
    assert_eq!(json["driver"], json!("mock"));
    let query = json["query"].as_str().unwrap();
    assert!(query.starts_with("where id = "), "{json}");
    assert!(query.ends_with("order by name desc"), "{json}");
}

#[test]
fn preview_engine_has_no_terminal_verbs() {
    for script in [
        r#"table("users").list()"#,
        r#"table("users").count()"#,
        r#"table("users").first()"#,
        r#"table("users").delete("1")"#,
    ] {
        let err = preview_script(script.into(), resolver()).expect_err(script);
        assert!(err.contains("Function not found"), "`{script}`: {err}");
    }
}

#[test]
fn preview_of_a_non_query_explains_itself() {
    let err = preview_script(r#""just a string""#.into(), resolver()).unwrap_err();
    assert!(err.contains("must end on the query itself"), "{err}");
}

#[test]
fn preview_traverses_a_reference_from_one_row() {
    let script = r#"table("users").where("id", "1").ref("posts")"#;
    let json = preview_script(script.into(), resolver()).unwrap();
    assert!(
        json["query"]
            .as_str()
            .unwrap()
            .starts_with("where author = "),
        "{json}"
    );
}
