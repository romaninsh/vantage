//! Edge cases of loading and registration: missing roots, default root
//! files that never parse, kept includes, root spellings, deleted includes
//! and conflicting registrations.

#[path = "support/note.rs"]
mod note;

use note::{NoteKind, SettingsKind, settings, write};
use vantage_inventory::{Depth, Document, Inventory, Kind, Location, Missing, OnFailure, Stage};

#[test]
fn load_on_missing_root_still_has_default_leaves() {
    let dir = tempfile::tempdir().unwrap();
    let mut inv = Inventory::new(dir.path().join("absent")).with(settings(Missing::Default));

    assert!(inv.load().is_err());
    assert_eq!(inv.keys_of("settings"), ["settings"]);
    assert!(inv.get::<SettingsKind>("settings").is_none());
}

#[test]
fn a_default_root_file_that_never_parses_keeps_its_leaf() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "settings.yaml", "colour: blue\n");

    let mut inv = Inventory::new(dir.path()).with(settings(Missing::Default));
    let failures = inv.load().unwrap();

    assert_eq!(inv.keys_of("settings"), ["settings"]);
    assert!(inv.get::<SettingsKind>("settings").is_none());
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].stage, Stage::Parse);
    assert_eq!(failures[0].kind, "settings");
}

#[test]
fn a_load_stage_report_keeps_the_includes() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "shared/t.txt", "Hello");
    write(
        dir.path(),
        "note/a.yaml",
        "title: !include ../shared/t.txt\n",
    );
    let kind = NoteKind {
        on_failure: |_, _| OnFailure::Report(vec!["broken".into()]),
        ..NoteKind::flat()
    };
    let mut inv = Inventory::new(dir.path()).with(kind);
    inv.load().unwrap();
    let shared = dir.path().join("shared/t.txt").canonicalize().unwrap();

    write(dir.path(), "note/a.yaml", "title: [no include parses\n");
    inv.reload_path(&dir.path().join("note/a.yaml"));

    let leaf = inv.leaf::<NoteKind>("a").unwrap();
    assert_eq!(leaf.errors, ["broken"]);
    assert_eq!(leaf.includes, std::slice::from_ref(&shared));
    // The kept edge still routes an edit of the include to the leaf.
    write(dir.path(), "shared/t.txt", "Bye");
    let changes = inv.reload_path(&shared).changes;
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].key, "a");
}

#[cfg(unix)]
#[test]
fn classify_accepts_either_spelling_of_the_root() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    let link = dir.path().join("link");
    write(&real, "note/a.yaml", "title: A\n");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let expected = Some(("note", "a".to_string()));

    for root in [&real, &link] {
        let inv = Inventory::new(root).with(NoteKind::flat());
        assert_eq!(inv.classify(&real.join("note/a.yaml")), expected);
        assert_eq!(inv.classify(&link.join("note/a.yaml")), expected);
        // A deleted file still classifies through its parent folder.
        assert_eq!(
            inv.classify(&link.join("note/gone.yaml")),
            Some(("note", "gone".to_string()))
        );
    }
}

#[cfg(unix)]
#[test]
fn a_deleted_include_reloads_its_dependents_through_a_symlinked_root() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    let link = dir.path().join("link");
    write(&real, "shared/t.txt", "Hello");
    write(&real, "note/a.yaml", "title: !include ../shared/t.txt\n");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut inv = Inventory::new(&link).with(NoteKind::flat());
    inv.load().unwrap();

    let include = link.join("shared/t.txt");
    std::fs::remove_file(&include).unwrap();
    let reload = inv.reload_path(&include);

    assert_eq!(reload.failures.len(), 1, "{:?}", reload.failures);
    assert_eq!(reload.failures[0].key, "a");
    assert_eq!(reload.failures[0].stage, Stage::Load);
}

#[test]
#[should_panic(expected = "inventory kind `note` registered twice")]
fn a_duplicate_kind_id_panics() {
    let _ = Inventory::new(".")
        .with(NoteKind::flat())
        .with(NoteKind::one_level());
}

#[test]
#[should_panic(expected = "kinds `note` and `memo` both read folder `note/`")]
fn two_kinds_on_one_folder_panic() {
    let _ = Inventory::new(".").with(NoteKind::flat()).with(NoteKind {
        id: "memo",
        ..NoteKind::flat()
    });
}

#[test]
#[should_panic(expected = "kinds `settings` and `other` both read root file `settings.yaml`")]
fn two_kinds_on_one_root_file_panic() {
    let _ = Inventory::new(".")
        .with(settings(Missing::Default))
        .with(SettingsKind {
            id: "other",
            file: "settings.yaml",
            missing: Missing::Absent,
        });
}

/// A kind whose value is the key it was parsed for.
struct KeyKind;

impl Kind for KeyKind {
    type Value = String;

    fn id(&self) -> &'static str {
        "keyed"
    }

    fn location(&self) -> Location {
        Location::Folder {
            name: "keyed",
            depth: Depth::OneLevel,
        }
    }

    fn parse(&self, key: &str, _doc: &Document) -> anyhow::Result<String> {
        Ok(key.to_string())
    }

    fn schema(&self) -> anyhow::Result<serde_json::Value> {
        Ok(serde_json::json!({}))
    }
}

#[test]
fn parse_is_given_the_key() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "keyed/a.yaml", "x: 1\n");
    let mut inv = Inventory::new(dir.path()).with(KeyKind);
    inv.load().unwrap();
    assert_eq!(inv.get::<KeyKind>("a").as_deref(), Some(&"a".to_string()));

    write(dir.path(), "keyed/sub/b.yaml", "x: 1\n");
    inv.reload_path(&dir.path().join("keyed/sub/b.yaml"));
    assert_eq!(
        inv.get::<KeyKind>("sub/b").as_deref(),
        Some(&"sub/b".to_string())
    );
}
