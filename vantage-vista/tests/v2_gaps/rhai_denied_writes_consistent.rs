//! With writes denied, every write verb must throw the denial message.
//!
//! Today `insert`, `upsert`, `patch` and `import_from` throw it, but the
//! table's `delete(id)` and `Record.delete()` return `false`, which a script
//! can't tell apart from a delete the backend doesn't support. In 2.0 a denied
//! or unsupported write always throws.

use vantage_vista::{Terminals, Writes};

use super::rhai_support::{host, mock, run};

const DENIED: &str = "writes are turned off here";

#[test]
#[ignore = "v2 phase 2: a denied delete throws like the other write verbs"]
fn rhai_denied_writes_consistent() {
    let shell = mock();
    let host = host(
        shell.clone(),
        Terminals::ReadWrite {
            limit: None,
            writes: Writes::Denied(DENIED.into()),
        },
    );

    let insert = run(&host, r#"table("t").insert(#{ id: "r9", a: 9 })"#);
    assert!(
        insert.as_ref().is_err_and(|e| e.contains(DENIED)),
        "insert must throw the denial, got {insert:?}"
    );

    let not_thrown: Vec<String> = [
        ("delete(id)", r#"table("t").delete("r1")"#),
        ("Record.delete()", r#"table("t").record("r1").delete()"#),
    ]
    .into_iter()
    .filter_map(|(verb, script)| match run(&host, script) {
        Err(e) if e.contains(DENIED) => None,
        other => Some(format!("{verb} -> {other:?}")),
    })
    .collect();
    assert!(
        not_thrown.is_empty(),
        "every denied delete must throw the denial like insert does: {not_thrown:?}"
    );
    assert!(shell.get_record("r1").is_some(), "r1 must not be deleted");
}
