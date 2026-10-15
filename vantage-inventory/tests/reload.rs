//! What a reload does when a file breaks, when validation fails, and how
//! boxed kinds register.

#[path = "support/note.rs"]
mod note;

use note::{NoteKind, write};
use vantage_inventory::{Change, Inventory, OnFailure, Stage};

fn notes(root: &std::path::Path, kind: NoteKind) -> Inventory {
    let mut inv = Inventory::new(root).with(kind);
    inv.load().unwrap();
    inv
}

#[test]
fn a_file_that_fails_on_first_load_adds_no_leaf() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: A\n");
    write(dir.path(), "note/syntax.yaml", "title: [unclosed\n");
    write(dir.path(), "note/schema.yaml", "titel: typo\n");

    let mut inv = Inventory::new(dir.path()).with(NoteKind::flat());
    let mut failures = inv.load().unwrap();

    assert_eq!(inv.keys_of("note"), ["a"]);
    failures.sort_by(|a, b| a.key.cmp(&b.key));
    assert_eq!(failures.len(), 2);
    assert_eq!(failures[0].key, "schema");
    assert_eq!(failures[0].stage, Stage::Parse);
    assert!(format!("{:#}", failures[0].error).contains("titel"));
    assert_eq!(failures[1].key, "syntax");
    assert_eq!(failures[1].stage, Stage::Load);
    assert_eq!(failures[1].path, dir.path().join("note/syntax.yaml"));
}

#[test]
fn a_broken_edit_keeps_the_last_good_value() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: Good\n");
    let mut inv = notes(dir.path(), NoteKind::flat());
    let path = dir.path().join("note/a.yaml");

    let mut stages = Vec::new();
    for broken in ["title: [broken\n", "nope: 1\n"] {
        write(dir.path(), "note/a.yaml", broken);
        let reload = inv.reload_path(&path);
        assert!(reload.changes.is_empty());
        stages.extend(reload.failures.iter().map(|f| f.stage));
    }

    let leaf = inv.leaf::<NoteKind>("a").unwrap();
    assert_eq!(leaf.value.unwrap().title, "Good");
    assert_eq!(leaf.serial, 1);
    assert!(leaf.errors.is_empty());
    assert_eq!(stages, [Stage::Load, Stage::Parse]);
}

#[test]
fn a_kind_can_report_a_broken_edit_on_the_kept_leaf() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: Good\n");
    let kind = NoteKind {
        on_failure: |stage, _| OnFailure::Report(vec![format!("broken at {stage:?}")]),
        ..NoteKind::flat()
    };
    let mut inv = notes(dir.path(), kind);
    let path = dir.path().join("note/a.yaml");

    write(dir.path(), "note/a.yaml", "nope: 1\n");
    let changes = inv.reload_path(&path).changes;
    assert_eq!(
        changes,
        [Change {
            kind: "note",
            key: "a".into(),
            serial: 1,
            removed: false
        }]
    );
    assert_eq!(inv.errors_for("note", "a"), ["broken at Parse"]);
    assert_eq!(inv.get::<NoteKind>("a").unwrap().title, "Good");

    // Re-saving the same broken text is deduplicated by its hash.
    write(dir.path(), "note/a.yaml", "nope: 1\n");
    assert!(inv.reload_path(&path).changes.is_empty());

    write(dir.path(), "note/a.yaml", "title: [x\n");
    inv.reload_path(&path);
    assert_eq!(inv.errors_for("note", "a"), ["broken at Load"]);

    // Restoring the last good text reloads and clears the report.
    write(dir.path(), "note/a.yaml", "title: Good\n");
    let changes = inv.reload_path(&path).changes;
    assert_eq!(changes[0].serial, 2);
    assert!(inv.errors_for("note", "a").is_empty());
}

#[test]
fn a_kind_can_drop_a_leaf_whose_edit_broke_it() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: Good\n");
    let kind = NoteKind {
        on_failure: |_, _| OnFailure::Remove,
        ..NoteKind::flat()
    };
    let mut inv = notes(dir.path(), kind);

    write(dir.path(), "note/a.yaml", "nope: 1\n");
    let changes = inv.reload_path(&dir.path().join("note/a.yaml")).changes;

    assert_eq!(changes.len(), 1);
    assert!(changes[0].removed);
    assert!(inv.keys_of("note").is_empty());
}

#[test]
fn paths_outside_any_kind_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: A\n");
    let mut inv = notes(dir.path(), NoteKind::flat());

    write(dir.path(), "other/a.yaml", "title: A\n");
    assert!(
        inv.reload_path(&dir.path().join("other/a.yaml"))
            .changes
            .is_empty()
    );
    assert_eq!(inv.classify(&dir.path().join("other/a.yaml")), None);
    assert_eq!(inv.classify(&dir.path().join("note/a.yml")), None);
    assert_eq!(
        inv.classify(&dir.path().join("note/a.yaml")),
        Some(("note", "a".to_string()))
    );
}
