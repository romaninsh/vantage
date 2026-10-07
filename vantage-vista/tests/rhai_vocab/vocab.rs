use vantage_vista::{Terminals, Writes};

use super::support::{host, run, store};

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
        r#"table("t").import_from(table("t"))"#,
    ] {
        let err = run(&host, script).unwrap_err();
        assert!(err.contains("writes are off"), "`{script}`: {err}");
    }
    let deleted = run(&host, r#"table("t").delete("r1")"#).unwrap();
    assert!(!deleted.as_bool().unwrap(), "a denied delete is `false`");
    assert!(
        run(&host, r#"table("t").count()"#).is_ok(),
        "reads still work"
    );
}

#[test]
fn read_host_has_no_write_verbs() {
    let host = host(&store(), Terminals::Read { limit: None });
    let err = run(&host, r#"table("t").insert(#{ a: 1 })"#).unwrap_err();
    assert!(err.contains("Function not found"), "{err}");
}

#[test]
fn describe_host_has_no_terminals() {
    let host = host(&store(), Terminals::Describe);
    for script in [r#"table("t").list()"#, r#"table("t").delete("r1")"#] {
        let err = run(&host, script).unwrap_err();
        assert!(err.contains("Function not found"), "`{script}`: {err}");
    }
}
