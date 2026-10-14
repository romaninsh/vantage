//! What `emit_schemas` reports and removes, and the errors that stop it.

#[path = "support/note.rs"]
mod note;

use note::{NoteKind, write};
use vantage_inventory::Inventory;

/// Restores a file's permissions when dropped, so a failed assertion still
/// leaves a tempdir that can be deleted.
#[cfg(unix)]
struct RestoreMode<'a>(&'a std::path::Path);

#[cfg(unix)]
impl Drop for RestoreMode<'_> {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o644));
    }
}

#[cfg(unix)]
#[test]
fn an_unreadable_yaml_file_keeps_old_schemas() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let inv = Inventory::new(dir.path()).with(NoteKind::flat());
    write(dir.path(), "note/note-schema-0.json", "{}");
    write(
        dir.path(),
        "note/a.yaml",
        "# yaml-language-server: $schema=./note-schema-0.json\ntitle: A\n",
    );
    let yaml = dir.path().join("note/a.yaml");
    std::fs::set_permissions(&yaml, std::fs::Permissions::from_mode(0o000)).unwrap();
    let _restore = RestoreMode(&yaml);
    if std::fs::read(&yaml).is_ok() {
        // Running as root: permissions do not stop the read.
        return;
    }

    assert!(inv.emit_schemas(1).is_err());
    assert!(dir.path().join("note/note-schema-0.json").exists());
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
