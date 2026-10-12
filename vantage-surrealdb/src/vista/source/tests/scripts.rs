//! Rhai on the shell: query-sourced vistas, the `surreal::` namespace and the
//! `surreal::me` anchor.

use vantage_rhai::{Block, Env};
use vantage_vista::{Capability, VistaFactory};

use super::{factory, spec};

#[tokio::test]
async fn query_sourced_vista_is_read_only_and_unwatchable() {
    let yaml = r#"
name: hv_clients
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
surreal:
  rhai: |
    surreal::select().from("client")
"#;
    let vista = factory().build_from_spec(spec(yaml)).expect("build");
    let caps = *vista.capabilities();
    for capability in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::ConfineWrites,
        Capability::Subscribe,
    ] {
        assert!(!caps.has(capability), "{capability}");
    }
    assert!(caps.has(Capability::NativeConditions));
    let err = vista.watch().await.err().expect("watch is refused");
    assert!(err.is_unsupported(), "{err:?}");
}

/// `surreal::me` is a namespace variable from `register_rhai_extensions`: a
/// script reads it with an empty `Env`, inside its own functions too.
#[test]
fn me_is_a_namespace_variable_of_the_script_host() {
    let host = factory().script_host();
    for code in [
        "surreal::me[\"name\"]",
        "fn anchor() { surreal::me[\"name\"] } anchor()",
    ] {
        let value = host
            .compile(&Block::from(code))
            .and_then(|script| script.eval(&Env::new()))
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert!(
            value.is::<crate::rhai_engine::RhaiExpr>(),
            "{code}: {}",
            value.type_name()
        );
    }
}

/// The vocabulary's free functions answer only under `surreal::`, so the
/// data verbs keep their names: `table(name)` is a table handle, and a bare
/// `ident` or `me` is unknown.
#[test]
fn free_functions_live_under_the_namespace() {
    let host = factory().script_host();
    let eval = |code: &str| {
        host.compile(&Block::from(code))
            .and_then(|script| script.eval(&Env::new()))
    };
    assert!(eval(r#"surreal::ident("name")"#).is_ok());
    for (code, expected) in [
        (r#"ident("name")"#, "Function not found: ident"),
        ("me", "unknown name `me` in scope"),
        (
            r#"surreal::table("client")"#,
            "Function not found: surreal::table",
        ),
    ] {
        let err = eval(code).expect_err(code).to_string();
        assert!(err.contains(expected), "{code}: {err}");
    }
}

#[test]
fn me_reads_inside_a_modify_script() {
    let yaml = r#"
name: client
columns:
  id: { type: thing, flags: [id] }
  name: { type: string }
surreal:
  table: client
  modify: |
    self.with_condition(surreal::me["name"] == "Marty")
"#;
    let vista = factory()
        .build_from_spec(spec(yaml))
        .expect("modify with surreal::me");
    let native = &vista.query().native;
    assert_eq!(native.len(), 1);
    assert!(native[0].contains("name"), "{native:?}");
}
