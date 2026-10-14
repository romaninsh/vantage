use std::sync::Arc;

use serde_json::json;
use vantage_vista::mocks::MockShell;
use vantage_vista::rhai::{Terminals, Writes};
use vantage_vista::{Vista, VistaCapabilities, VistaResolver};

use super::support::{host, host_with, json, read_write_terminals, rec, run, store};

#[test]
fn denied_writes_throw_the_message() {
    let terminals = Terminals::ReadWrite {
        limit: None,
        writes: Writes::Denied("writes are off".into()),
    };
    let host = host(&store(), terminals);
    for script in [
        r#"table("t").insert(#{ a: 1 })"#,
        r#"table("t").upsert("r1", #{ a: 1 })"#,
        r#"table("t").patch("r1", #{ a: 1 })"#,
        r#"table("t").delete("r1")"#,
        r#"table("t").import_from(table("t"))"#,
        r#"table("t").record("r1").delete()"#,
    ] {
        let err = run(&host, script).unwrap_err();
        assert!(err.contains("writes are off"), "`{script}`: {err}");
    }
    assert_eq!(json(&host, r#"table("t").count()"#), json!(3), "reads work");
}

/// The `mock` table advertises no `Import`, so an import into it is refused.
#[test]
fn an_import_the_vista_refuses_throws() {
    let host = host(&store(), read_write_terminals());
    for script in [
        r#"table("mock").import_from(table("t"))"#,
        r#"table("mock").import_from(table("t"), #{ id: "${row.id}" })"#,
    ] {
        let err = run(&host, script).unwrap_err();
        assert!(
            err.contains("capability is not supported"),
            "`{script}`: {err}"
        );
        assert!(err.contains("import"), "`{script}`: {err}");
    }
    assert_eq!(json(&host, r#"table("mock").ids()"#), json!(["r1"]));
}

/// Over a read-only mock, both deletes throw the Vista's refusal.
#[test]
fn a_delete_the_vista_refuses_throws() {
    let shell = MockShell::new()
        .with_capabilities(VistaCapabilities::new())
        .with_record("r1", rec(&[("a", 1)]));
    let resolver: Arc<dyn VistaResolver> =
        Arc::new(move |name: &str| Ok(Vista::new(name, shell.clone())));
    let host = host_with(resolver, read_write_terminals());
    for script in [
        r#"table("t").delete("r1")"#,
        r#"table("t").record("r1").delete()"#,
    ] {
        let err = run(&host, script).unwrap_err();
        assert!(
            err.contains("capability is not supported"),
            "`{script}`: {err}"
        );
        assert!(err.contains("delete"), "`{script}`: {err}");
    }
    assert_eq!(json(&host, r#"table("t").ids()"#), json!(["r1"]));
}
