//! What `emit_schemas` reports, and that a kind's schema error stops it.

#[path = "support/note.rs"]
mod note;

use note::{NoteKind, write};
use vantage_inventory::Inventory;

#[test]
fn emission_reports_written_and_removed_files() {
    let dir = tempfile::tempdir().unwrap();
    let inv = Inventory::new(dir.path()).with(NoteKind::flat());
    write(dir.path(), "note/note-schema-0.json", "{}");
    let folder = dir.path().join("note");

    let report = inv.emit_schemas(1).unwrap();
    assert_eq!(
        report.written,
        [folder.join("README.md"), folder.join("note-schema-1.json")]
    );
    assert_eq!(report.removed, [folder.join("note-schema-0.json")]);

    // Nothing to do the second time.
    let report = inv.emit_schemas(1).unwrap();
    assert!(report.written.is_empty(), "{report:?}");
    assert!(report.removed.is_empty(), "{report:?}");
}

#[test]
fn a_schema_error_fails_emission() {
    let dir = tempfile::tempdir().unwrap();
    let inv = Inventory::new(dir.path()).with(NoteKind {
        broken_schema: true,
        ..NoteKind::flat()
    });

    let err = inv.emit_schemas(1).unwrap_err();

    assert!(
        format!("{err:#}").contains("note schema unavailable"),
        "{err:#}"
    );
    assert!(!dir.path().join("note/note-schema-1.json").exists());
}
