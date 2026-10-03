use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::Env;
use vantage_types::Record;

use super::narrow::{describe_host, mock_resolver, users_vista};
use crate::rhai::bridge::block_on;
use crate::rhai::{
    augment_source_closure, eval_modify_script, eval_ref_script, lazy_value_closure,
};
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
    let err = eval_ref_script(&host, r#"table("ghosts")"#, Env::new(), &row(&[]))
        .err()
        .expect("unknown table");
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
fn modify_script_without_a_handle_returns_self() {
    let host = describe_host(mock_resolver());
    let vista = eval_modify_script(&host, "let x = 1;", users_vista()).unwrap();
    assert_eq!(ids(&vista).len(), 3);
}

#[test]
fn augment_closure_narrows_base_per_row() {
    let f = augment_source_closure(mock_resolver(), r#"self.where("id", row.key)"#.into());
    let narrowed = f(&row(&[("key", text("2"))]), users_vista()).unwrap();
    assert_eq!(ids(&narrowed), ["2"]);
    let narrowed = f(&row(&[("key", text("3"))]), users_vista()).unwrap();
    assert_eq!(ids(&narrowed), ["3"]);
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
