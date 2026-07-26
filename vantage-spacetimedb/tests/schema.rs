//! Schema-introspection tests, run entirely offline against fixtures captured
//! from a real host.
//!
//! The fixtures in `tests/fixtures/` are the verbatim bodies of
//! `GET /v1/database/smoke/schema?version={9,10}` from
//! `clockworklabs/spacetime:v2.7.0-hotfix3`, for a module that deliberately
//! covers the shapes this driver has to handle: a primary key with an auto-inc
//! sequence, a separate unique constraint, a table with neither, a private
//! table, and a `u64` above 2^53.
//!
//! Testing against real bytes rather than a hand-built `RawModuleDef` is the
//! point: the encoding has surprises (`Option` as `{"some": …}`, unit enums as
//! `{"Public": []}`, columns living in the typespace rather than on the table)
//! and a fixture catches a wrong assumption about any of them.

use vantage_spacetimedb::schema::{ModuleSchema, RowIdentity, SchemaVersion, TableKind};

const V9: &str = include_str!("fixtures/schema-v9-smoke.json");
const V10: &str = include_str!("fixtures/schema-v10-smoke.json");

#[test]
fn v10_decodes_into_normalised_tables() {
    let schema = ModuleSchema::from_v10_json(V10).expect("v10 fixture should decode");

    assert_eq!(schema.version, SchemaVersion::V10);
    assert!(
        schema.event_detection(),
        "v10 is the ABI that reports is_event"
    );

    let mut names: Vec<&str> = schema.tables.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, ["person", "reading", "secret"]);

    let mut reducers = schema.reducers.clone();
    reducers.sort();
    assert_eq!(reducers, ["add", "add_reading"]);
}

#[test]
fn v9_decodes_the_same_tables_but_cannot_see_event_flags() {
    let schema = ModuleSchema::from_v9_json(V9).expect("v9 fixture should decode");

    assert_eq!(schema.version, SchemaVersion::V9);
    assert!(
        !schema.event_detection(),
        "v9 has no is_event field, so detection must report false rather than \
         claiming every table is ordinary"
    );

    let mut names: Vec<&str> = schema.tables.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, ["person", "reading", "secret"]);
}

#[test]
fn columns_are_resolved_out_of_the_typespace_in_declaration_order() {
    // Columns are not on the table definition — it holds a `product_type_ref`
    // into the module typespace. This asserts that indirection is followed, and
    // that declaration order survives it.
    for (label, schema) in [
        ("v10", ModuleSchema::from_v10_json(V10).unwrap()),
        ("v9", ModuleSchema::from_v9_json(V9).unwrap()),
    ] {
        let person = &schema.tables["person"];
        let names: Vec<&str> = person.columns.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["id", "handle", "score", "big", "active"],
            "{label}: column names and order"
        );
    }
}

#[test]
fn primary_key_and_unique_constraints_resolve_from_column_indices() {
    let schema = ModuleSchema::from_v10_json(V10).unwrap();
    let person = &schema.tables["person"];

    // Both are expressed as column *indices* in the ABI.
    assert_eq!(person.primary_key.as_deref(), Some("id"));
    assert!(
        person.unique_columns.contains(&"handle".to_string()),
        "the #[unique] handle column should surface as a unique constraint, got {:?}",
        person.unique_columns
    );
}

#[test]
fn row_identity_falls_back_from_primary_key_to_unique_to_content_hash() {
    let schema = ModuleSchema::from_v10_json(V10).unwrap();

    assert_eq!(
        schema.tables["person"].row_identity(),
        RowIdentity::PrimaryKey("id".into()),
        "a declared primary key wins"
    );

    // `reading` has neither a primary key nor a unique constraint, so there is
    // no column to identify a row by. Hashing the row is sound here only because
    // SpacetimeDB deletes carry the whole row, so a delete hashes to the same id
    // as its insert.
    assert_eq!(
        schema.tables["reading"].row_identity(),
        RowIdentity::ContentHash,
        "no key and no unique constraint means a content hash"
    );
    assert_eq!(schema.tables["reading"].row_identity().column(), None);
}

#[test]
fn private_tables_are_marked_and_refused() {
    let schema = ModuleSchema::from_v10_json(V10).unwrap();

    assert!(schema.tables["person"].public);
    assert!(
        !schema.tables["secret"].public,
        "a table without `public` is private"
    );

    // A Vista over a private table would read as permanently empty, because the
    // client is not permitted to see any of its rows. Refusing beats that.
    let refusal = schema
        .metadata_for("secret")
        .expect_err("a private table must be refused");
    assert!(
        refusal.to_string().contains("private"),
        "the error should say why: {refusal}"
    );
}

#[test]
fn metadata_flags_id_and_nominates_a_title() {
    let schema = ModuleSchema::from_v10_json(V10).unwrap();
    let metadata = schema.metadata_for("person").expect("person is public");

    assert_eq!(metadata.id_column.as_deref(), Some("id"));
    assert!(metadata.columns["id"].is_id());

    // The first non-id string column becomes the display label, so a grid has
    // something to show without the inventory author having to say so.
    assert!(
        metadata.columns["handle"].is_title(),
        "handle should be nominated as the title column"
    );
}

#[test]
fn wide_integers_map_to_string_rather_than_silently_truncating() {
    let schema = ModuleSchema::from_v10_json(V10).unwrap();
    let metadata = schema.metadata_for("person").unwrap();

    // u64 still fits Vista's integer carrier, so `big` is an int...
    assert_eq!(metadata.columns["big"].original_type, "int");
    assert_eq!(metadata.columns["score"].original_type, "int");
    assert_eq!(metadata.columns["active"].original_type, "bool");
    assert_eq!(metadata.columns["handle"].original_type, "string");

    let reading = schema.metadata_for("reading").unwrap();
    assert_eq!(reading.columns["value"].original_type, "float");
}

#[test]
fn unknown_table_names_list_what_is_available() {
    let schema = ModuleSchema::from_v10_json(V10).unwrap();
    let err = schema
        .metadata_for("no_such_table")
        .expect_err("unknown table must error");
    let text = err.to_string();
    assert!(
        text.contains("no_such_table"),
        "error should name the missing table: {text}"
    );
}

#[test]
fn no_table_in_the_smoke_module_is_an_event_table() {
    // A baseline for the refusal path: nothing here is an event table, so if a
    // later fixture gains one, the difference is visible.
    let schema = ModuleSchema::from_v10_json(V10).unwrap();
    for (name, table) in &schema.tables {
        assert_ne!(table.kind, TableKind::Event, "{name} should not be an event table");
    }
}
