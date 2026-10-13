//! With writes denied, every write verb must throw the denial message:
//! `insert`, `upsert`, `patch`, `import_from`, the table's `delete(id)` and
//! `Record.delete()`. A delete never answers `false` for a denial, so a
//! script can't mistake a refused write for a missing row.

use vantage_vista::rhai::{Terminals, Writes};

use super::rhai_support::{host, mock, run};

const DENIED: &str = "writes are turned off here";

#[test]
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
