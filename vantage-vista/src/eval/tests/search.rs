//! The search rule: a case-insensitive substring of a searched cell's text.

use super::*;

fn search(row: &Record<CborValue>, needle: &str) -> bool {
    VistaQuery::new()
        .with_search(needle)
        .matches(row, &schemaless())
        .unwrap()
}

#[test]
fn text_matches_case_insensitively() {
    let r = sample();
    assert!(search(&r, "STRAßE") && search(&r, "raß"));
    assert!(!search(&r, "absent"));
}

#[test]
fn numbers_and_bools_match_as_their_text() {
    let r = row(&[
        ("qty", int(1234)),
        ("price", CborValue::Float(4.95)),
        ("vip", CborValue::Bool(true)),
    ]);
    assert!(search(&r, "23") && search(&r, "4.9") && search(&r, "TRUE"));
    assert!(!search(&r, "false") && !search(&r, "99"));
}

/// A SurrealDB record id is `Tag(8, [table, key])`: both halves are read.
#[test]
fn a_tagged_value_matches_through_its_content() {
    let id = CborValue::Tag(
        8,
        Box::new(CborValue::Array(vec![text("client"), text("Marty")])),
    );
    let r = row(&[("owner", id)]);
    assert!(search(&r, "client") && search(&r, "marty"));
}

#[test]
fn nested_maps_and_arrays_match_at_any_depth() {
    let owner = map(&[
        ("name", text("Doc")),
        (
            "phones",
            CborValue::Array(vec![map(&[("home", text("555-0199"))])]),
        ),
    ]);
    let r = row(&[("owner", owner)]);
    assert!(search(&r, "doc") && search(&r, "0199"));
    assert!(!search(&r, "home"), "map keys are not searched");
}

#[test]
fn null_and_bytes_hold_no_text() {
    let r = row(&[
        ("a", CborValue::Null),
        ("b", CborValue::Bytes(b"null".to_vec())),
    ]);
    assert!(!search(&r, "null"));
}

#[test]
fn an_empty_needle_matches_every_row() {
    assert!(search(&Record::new(), ""));
    assert!(search(&row(&[("n", int(1))]), ""));
}

fn search_in(row: &Record<CborValue>, metadata: &VistaMetadata, needle: &str) -> bool {
    VistaQuery::new()
        .with_search(needle)
        .matches(row, metadata)
        .unwrap()
}

fn note_row() -> Record<CborValue> {
    row(&[("name", text("Marty")), ("note", text("hoverboard"))])
}

#[test]
fn search_reads_only_searchable_columns() {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Searchable))
        .with_column(Column::new("note", "String"));
    assert!(search_in(&note_row(), &metadata, "mar"));
    assert!(!search_in(&note_row(), &metadata, "hover"));
}

#[test]
fn search_reads_every_column_when_none_is_searchable() {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("note", "String"));
    assert!(search_in(&note_row(), &metadata, "mar"));
    assert!(search_in(&note_row(), &metadata, "hover"));
}

#[test]
fn search_columns_are_the_flagged_ones_or_every_column() {
    let plain = VistaMetadata::new()
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("note", "String"));
    assert_eq!(plain.search_columns(), ["name", "note"]);
    let flagged =
        plain.with_column(Column::new("title", "String").with_flag(ColumnFlag::Searchable));
    assert_eq!(flagged.search_columns(), ["title"]);
}
