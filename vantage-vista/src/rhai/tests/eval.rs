use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::Env;
use vantage_types::Record;

use super::narrow::{describe_host, mock_resolver, users_vista};
use crate::rhai::bridge::block_on;
use crate::rhai::{eval_modify_script, eval_ref_script, lazy_value_closure};
use crate::vista::Vista;

fn row(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn ids(vista: &Vista) -> Vec<String> {
    block_on(vista.list_values())
        .unwrap()
        .unwrap()
        .into_keys()
        .collect()
}

#[test]
fn ref_script_narrows_target_with_literal_condition() {
    let host = describe_host(mock_resolver());
    let script = r#"table("users").where("vip_flag", true)"#;
    let vista = eval_ref_script(&host, script, Env::new(), &row(&[])).unwrap();
    assert_eq!(ids(&vista), ["1", "3"]);
}

#[test]
fn ref_script_can_read_the_parent_row() {
    let host = describe_host(mock_resolver());
    let script = r#"table("users").where("id", row.id)"#;
    let vista = eval_ref_script(&host, script, Env::new(), &row(&[("id", text("3"))])).unwrap();
    assert_eq!(ids(&vista), ["3"]);
}

#[test]
fn ref_script_unknown_table_surfaces_resolver_error() {
    let host = describe_host(mock_resolver());
    let Err(err) = eval_ref_script(&host, r#"table("ghosts")"#, Env::new(), &row(&[])) else {
        panic!("unknown table should fail");
    };
    assert!(err.to_string().contains("unknown table"), "{err}");
}

#[test]
fn modify_script_tweaks_an_existing_vista() {
    let host = describe_host(mock_resolver());
    let vista =
        eval_modify_script(&host, r#"self.where("vip_flag", true)"#, users_vista()).unwrap();
    assert_eq!(ids(&vista), ["1", "3"]);
}

#[test]
fn modify_script_of_self_alone_keeps_the_vista() {
    let host = describe_host(mock_resolver());
    let vista = eval_modify_script(&host, "self", users_vista()).unwrap();
    assert_eq!(ids(&vista).len(), 3);
}

#[test]
fn modify_script_must_end_on_a_handle() {
    let host = describe_host(mock_resolver());
    for script in [
        "let x = 1;",
        "42",
        r#"if false { self.where("vip_flag", true) }"#,
    ] {
        let err = eval_modify_script(&host, script, users_vista()).unwrap_err();
        assert!(err.is_incorrect_usage(), "`{script}`: {err}");
        let text = err.to_string();
        assert!(text.contains("must end on a table handle"), "{text}");
        assert!(text.contains("rhai modify script"), "{text}");
    }
}

fn discard_error(script: &str) -> String {
    let host = describe_host(mock_resolver());
    let err = eval_modify_script(&host, script, users_vista()).unwrap_err();
    assert!(err.is_incorrect_usage(), "`{script}`: {err}");
    let text = err.to_string();
    assert!(
        text.contains("table handle is discarded"),
        "`{script}`: {text}"
    );
    text
}

#[test]
fn a_dropped_handle_is_refused_when_the_script_compiles() {
    let text = discard_error(r#"self.where("vip_flag", true); self.where("id", "2");"#);
    assert!(text.contains("line: 1"), "{text}");
    let text = discard_error("let n = 1;\nself.where(\"id\", \"2\");\nself");
    assert!(text.contains("line: 2"), "{text}");
}

/// The rule is the verb a statement ends on, not what the chain starts
/// from, so narrowing an alias of `self` and dropping it is refused too.
#[test]
fn a_dropped_handle_on_an_alias_of_self_is_refused() {
    discard_error(r#"let x = self; x.where("vip_flag", true); x"#);
    discard_error(r#"let t = table("users"); t.sort("name"); t"#);
}

#[test]
fn a_dropped_handle_in_a_branch_is_refused() {
    discard_error(
        r#"if true { self.where("vip_flag", true); self } else { self.where("id", "2") }"#,
    );
    discard_error(r#"if true { if true { self.where("id", "2"); 1 } else { 2 }; } self"#);
}

#[test]
fn handles_in_value_position_or_assigned_are_kept() {
    let host = describe_host(mock_resolver());
    for (script, want) in [
        (r#"self.where("id", "2");"#, vec!["2"]),
        (
            r#"let vip = self.where("vip_flag", true); vip"#,
            vec!["1", "3"],
        ),
        (
            r#"if true { self.where("id", "2") } else { self.where("id", "3") }"#,
            vec!["2"],
        ),
    ] {
        let vista = eval_modify_script(&host, script, users_vista()).unwrap();
        assert_eq!(ids(&vista), want, "{script}");
    }
}

#[test]
fn a_ref_script_dropping_a_table_handle_is_refused() {
    let host = describe_host(mock_resolver());
    let script = r#"table("users").where("vip_flag", true); table("users")"#;
    let err = eval_ref_script(&host, script, Env::new(), &row(&[])).unwrap_err();
    assert!(
        err.to_string().contains("table handle is discarded"),
        "{err}"
    );
}

#[test]
fn lazy_closure_compiles_once_and_fails_early() {
    let f = lazy_value_closure("row.n * 2").unwrap();
    let r = row(&[("n", CborValue::Integer(21.into()))]);
    assert_eq!(f(&r).unwrap(), CborValue::Integer(42.into()));
    assert!(
        lazy_value_closure("row.n *").is_err(),
        "syntax fails at build"
    );
}

#[test]
fn lazy_expression_is_bounded() {
    let f = lazy_value_closure("loop {}").unwrap();
    let err = f(&row(&[])).unwrap_err();
    assert!(err.to_string().contains("limit"), "{err}");
}
