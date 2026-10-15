//! Loading a project folder through the registry: walk, keys, cross-reference
//! validation, include dependents, deletes, root files and schema emission.

#[path = "support/note.rs"]
mod note;

use note::{NOTE_README, NoteKind, SettingsKind, write};
use vantage_inventory::{Change, Inventory, Kind, Missing};

fn notes(root: &std::path::Path) -> Inventory {
    let mut inv = Inventory::new(root).with(NoteKind::flat());
    inv.load().unwrap();
    inv
}

#[test]
fn loads_each_file_as_a_leaf() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/b.yaml", "title: Bee\n");
    write(dir.path(), "note/a.yaml", "title: Ay\n");
    write(dir.path(), "note/README.md", "# not a leaf\n");
    write(dir.path(), "note/c.txt", "title: not yaml by name\n");

    let inv = notes(dir.path());

    assert_eq!(inv.keys_of("note"), ["a", "b"]);
    assert_eq!(inv.get::<NoteKind>("a").unwrap().title, "Ay");
    assert_eq!(inv.get::<NoteKind>("b").unwrap().title, "Bee");
    let leaf = inv.leaf::<NoteKind>("a").unwrap();
    assert_eq!(leaf.serial, 1);
    assert!(leaf.errors.is_empty());
    assert_eq!(leaf.path, dir.path().join("note/a.yaml"));
}

#[test]
fn cross_reference_errors_name_the_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: A\nrefers_to: zzz\n");
    write(dir.path(), "note/b.yaml", "title: B\nrefers_to: a\n");

    let inv = notes(dir.path());

    assert_eq!(
        inv.errors_for("note", "a"),
        ["a: refers_to `zzz` is not a note"]
    );
    assert!(inv.errors_for("note", "b").is_empty());
    assert_eq!(
        inv.snapshot().index("referrers").unwrap().get("a").unwrap(),
        &["b"]
    );
}

#[test]
fn include_change_reloads_dependents() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "shared/t.txt", "Hello");
    write(
        dir.path(),
        "note/a.yaml",
        "title: !include ../shared/t.txt\n",
    );
    write(dir.path(), "note/b.yaml", "title: Plain\n");
    let mut inv = notes(dir.path());
    assert_eq!(inv.get::<NoteKind>("a").unwrap().title, "Hello");

    write(dir.path(), "shared/t.txt", "Goodbye");
    let changes = inv.reload_path(&dir.path().join("shared/t.txt")).changes;

    assert_eq!(
        changes,
        [Change {
            kind: "note",
            key: "a".into(),
            serial: 2,
            removed: false
        }]
    );
    assert_eq!(inv.get::<NoteKind>("a").unwrap().title, "Goodbye");
    assert_eq!(inv.leaf::<NoteKind>("b").unwrap().serial, 1);
}

#[test]
fn deleting_a_file_removes_its_leaf() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: A\nrefers_to: b\n");
    write(dir.path(), "note/b.yaml", "title: B\n");
    let mut inv = notes(dir.path());
    assert!(inv.errors_for("note", "a").is_empty());

    let path = dir.path().join("note/b.yaml");
    std::fs::remove_file(&path).unwrap();
    let changes = inv.reload_path(&path).changes;

    assert_eq!(
        changes,
        [Change {
            kind: "note",
            key: "b".into(),
            serial: 1,
            removed: true
        }]
    );
    assert_eq!(inv.keys_of("note"), ["a"]);
    assert!(inv.get::<NoteKind>("b").is_none());
    // The referrer is re-checked against the shrunken key set.
    assert_eq!(
        inv.errors_for("note", "a"),
        ["a: refers_to `b` is not a note"]
    );
}

#[test]
fn one_level_subfolders_key_as_sub_slash_stem() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/top.yaml", "title: Top\n");
    write(dir.path(), "note/x/y.yaml", "title: Nested\n");
    write(dir.path(), "note/x/deeper/w.yaml", "title: Too deep\n");

    let mut inv = Inventory::new(dir.path()).with(NoteKind::one_level());
    inv.load().unwrap();
    assert_eq!(inv.keys_of("note"), ["top", "x/y"]);
    assert_eq!(inv.get::<NoteKind>("x/y").unwrap().title, "Nested");

    write(dir.path(), "note/x/y.yaml", "title: Edited\n");
    let changes = inv.reload_path(&dir.path().join("note/x/y.yaml")).changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].key, "x/y");
    assert!(
        inv.reload_path(&dir.path().join("note/x/deeper/w.yaml"))
            .changes
            .is_empty()
    );
    assert_eq!(inv.classify(&dir.path().join("note/x/deeper/w.yaml")), None);

    // A flat kind does not descend at all.
    let flat = notes(dir.path());
    assert_eq!(flat.keys_of("note"), ["top"]);
    assert_eq!(flat.classify(&dir.path().join("note/x/y.yaml")), None);
}

#[test]
fn root_file_missing_uses_default_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let mut inv = Inventory::new(dir.path())
        .with(SettingsKind {
            id: "settings",
            file: "settings.yaml",
            missing: Missing::Default,
        })
        .with(SettingsKind {
            id: "extra",
            file: "extra.yaml",
            missing: Missing::Absent,
        });
    inv.load().unwrap();

    // Default: the leaf exists with no value, so callers use their default.
    // `get` finds the first registered `SettingsKind`, which is `settings`.
    let value = |inv: &Inventory| inv.get::<SettingsKind>("settings");
    assert_eq!(inv.keys_of("settings"), ["settings"]);
    assert!(value(&inv).is_none());
    // Absent: no leaf at all.
    assert!(inv.keys_of("extra").is_empty());

    let path = dir.path().join("settings.yaml");
    write(dir.path(), "settings.yaml", "theme: dark\n");
    let changes = inv.reload_path(&path).changes;
    assert_eq!(changes.len(), 1);
    assert!(!changes[0].removed);
    assert_eq!(value(&inv).unwrap().theme, "dark");

    // A broken file reverts to the default too, rather than keeping the
    // last good value.
    write(dir.path(), "settings.yaml", "theme: [dark\n");
    let changes = inv.reload_path(&path).changes;
    assert_eq!(changes.len(), 1);
    assert!(value(&inv).is_none());

    // Deleting it reverts to the default instead of removing the leaf.
    write(dir.path(), "settings.yaml", "theme: light\n");
    inv.reload_path(&path);
    assert!(value(&inv).is_some());
    std::fs::remove_file(&path).unwrap();
    let changes = inv.reload_path(&path).changes;
    assert_eq!(changes.len(), 1);
    assert!(!changes[0].removed);
    assert_eq!(inv.keys_of("settings"), ["settings"]);
    assert!(value(&inv).is_none());
}

#[test]
fn rewriting_a_file_with_identical_content_reports_no_change() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: A\n");
    let mut inv = notes(dir.path());

    write(dir.path(), "note/a.yaml", "title: A\n");
    let changes = inv.reload_path(&dir.path().join("note/a.yaml")).changes;

    assert!(changes.is_empty(), "{changes:?}");
    assert_eq!(inv.leaf::<NoteKind>("a").unwrap().serial, 1);
}

#[test]
fn a_clean_edit_bumps_the_serial_and_is_reported_with_it() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: A\n");
    let mut inv = notes(dir.path());

    write(dir.path(), "note/a.yaml", "title: A2\n");
    let changes = inv.reload_path(&dir.path().join("note/a.yaml")).changes;

    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].serial, 2);
    assert_eq!(inv.leaf::<NoteKind>("a").unwrap().serial, 2);
    assert_eq!(inv.get::<NoteKind>("a").unwrap().title, "A2");
}

#[test]
fn boxed_kinds_register_in_order_and_read_back_typed() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "note/a.yaml", "title: Boxed\n");
    let kinds: Vec<Box<dyn vantage_inventory::DynKind>> = vec![Box::new(NoteKind::flat())];

    let mut inv = Inventory::new(dir.path()).with_all(kinds);
    inv.load().unwrap();

    assert_eq!(inv.get::<NoteKind>("a").unwrap().title, "Boxed");
    assert_eq!(inv.kinds().map(|k| k.id()).collect::<Vec<_>>(), ["note"]);
}

#[test]
fn schema_files_use_versioned_names() {
    let dir = tempfile::tempdir().unwrap();
    let inv = Inventory::new(dir.path()).with(NoteKind::flat());
    write(dir.path(), "note/note-schema-0.json", "{}");

    inv.emit_schemas(1).unwrap();

    let v1 = dir.path().join("note/note-schema-1.json");
    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&v1).unwrap()).unwrap();
    assert_eq!(parsed, NoteKind::flat().schema().unwrap());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note/README.md")).unwrap(),
        NOTE_README
    );
    // Not current and referenced by nothing.
    assert!(!dir.path().join("note/note-schema-0.json").exists());

    // A file still pointing at version 1 keeps it alive past a bump.
    write(
        dir.path(),
        "note/a.yaml",
        "# yaml-language-server: $schema=./note-schema-1.json\ntitle: A\n",
    );
    inv.emit_schemas(2).unwrap();
    assert!(dir.path().join("note/note-schema-2.json").exists());
    assert!(v1.exists());

    write(dir.path(), "note/a.yaml", "title: A\n");
    inv.emit_schemas(2).unwrap();
    assert!(!v1.exists());
}
