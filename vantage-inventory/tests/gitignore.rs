//! The managed `.gitignore` block: written once, replaced in place, and
//! never touching the project's own entries.

use vantage_inventory::{GITIGNORE_BEGIN, GITIGNORE_END, ensure_gitignore};

const LINES: &[&str] = &["# generated", "*/*-schema-*.json", "", ".cache/"];

fn read(dir: &tempfile::TempDir) -> String {
    std::fs::read_to_string(dir.path().join(".gitignore")).unwrap()
}

#[test]
fn writing_twice_leaves_one_block() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        ensure_gitignore(dir.path(), LINES).unwrap(),
        "first call writes"
    );
    let once = read(&dir);
    assert!(
        !ensure_gitignore(dir.path(), LINES).unwrap(),
        "second call does not"
    );

    assert_eq!(once, read(&dir));
    assert_eq!(read(&dir).matches(GITIGNORE_BEGIN).count(), 1);
}

#[test]
fn an_existing_gitignore_keeps_its_own_entries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".gitignore"), "# mine\nsecrets.txt\n").unwrap();

    ensure_gitignore(dir.path(), LINES).unwrap();

    assert_eq!(
        read(&dir),
        format!(
            "# mine\nsecrets.txt\n\n{GITIGNORE_BEGIN}\n# generated\n*/*-schema-*.json\n\n.cache/\n{GITIGNORE_END}\n"
        )
    );
}

#[test]
fn an_unreadable_gitignore_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".gitignore");
    let bytes = b"secrets.txt\n\xff\xfe not utf-8\n";
    std::fs::write(&path, bytes).unwrap();

    assert!(ensure_gitignore(dir.path(), LINES).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
