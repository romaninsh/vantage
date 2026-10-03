use serde_json::json;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::Env;
use vantage_vista::rhai::block_on;
use vantage_vista::{Terminals, Vista, eval_modify_script, eval_ref_script, preview_script};

use super::support::{host, rec, resolver, store};

fn ids(vista: &Vista) -> Vec<String> {
    block_on(vista.list_values())
        .unwrap()
        .unwrap()
        .into_keys()
        .collect()
}

#[test]
fn preview_renders_resolved_query() {
    let script = r#"table("t").where("a", 1).sort("n", "desc")"#;
    let json = preview_script(script.into(), resolver(&store())).unwrap();
    assert_eq!(json["driver"], json!("memory"));
    assert_eq!(json["table"], json!("t"));
    assert_eq!(json["order"], json!([["n", "desc"]]));
    assert_eq!(json["conditions"].as_array().unwrap().len(), 1);
}

#[test]
fn modify_script_narrows_self() {
    let store = store();
    let host = host(&store, Terminals::Describe);
    let base = resolver(&store)("t").unwrap();
    let vista = eval_modify_script(&host, r#"self.where("a", 1).sort("n")"#, base).unwrap();
    assert_eq!(ids(&vista), ["r3", "r1"]);
}

/// A Describe host over `store` with extension verbs `only_a1` and
/// `only_n2` that narrow the Vista in hand to `a == 1` / `n == 2`.
fn host_with_extension(store: &vantage_memory::MemoryStore) -> vantage_rhai::Host {
    use vantage_rhai::rhai::EvalAltResult;
    use vantage_vista::{CborValue, DataVocab, FilterOp, Handle};
    vantage_rhai::Host::builder(vantage_rhai::Limits::background())
        .vocab_fn(|engine| {
            for (verb, col, value) in [("only_a1", "a", 1), ("only_n2", "n", 2)] {
                engine.register_fn(
                    verb,
                    move |h: &mut Handle| -> Result<Handle, Box<EvalAltResult>> {
                        h.with_base_vista(verb, |v| {
                            v.add_condition(col, FilterOp::Eq, CborValue::Integer(value.into()))
                        })
                        .map_err(|e| e.to_string().into())
                    },
                );
            }
        })
        .vocab(DataVocab {
            resolver: Some(resolver(store)),
            terminals: Terminals::Describe,
        })
        .build()
}

#[test]
fn extension_verb_leaves_stored_handle_unchanged() {
    let store = store();
    let host = host_with_extension(&store);
    let base = || resolver(&store)("t").unwrap();
    let all = eval_modify_script(
        &host,
        "let all = self; let mine = all.only_a1(); all",
        base(),
    )
    .unwrap();
    assert_eq!(ids(&all), ["r1", "r2", "r3"]);
    let mine = eval_modify_script(&host, "self.only_a1()", base()).unwrap();
    assert_eq!(ids(&mine), ["r1", "r3"]);
}

#[test]
fn extension_verb_as_a_statement_still_applies() {
    let store = store();
    let host = host_with_extension(&store);
    let base = || resolver(&store)("t").unwrap();
    let modified = eval_modify_script(&host, "self.only_a1();", base()).unwrap();
    assert_eq!(ids(&modified), ["r1", "r3"]);
    let augmented =
        vantage_vista::eval_augment_source(&host, "self.only_a1();", base(), &rec(&[])).unwrap();
    assert_eq!(ids(&augmented), ["r1", "r3"]);
}

#[test]
fn two_extension_statements_both_apply() {
    let store = store();
    let host = host_with_extension(&store);
    let base = || resolver(&store)("t").unwrap();
    let modified = eval_modify_script(&host, "self.only_a1(); self.only_n2();", base()).unwrap();
    assert_eq!(ids(&modified), ["r3"]);
}

#[test]
fn ref_script_builds_target_from_row() {
    let store = store();
    let host = host(&store, Terminals::Describe);
    let row = rec(&[("a", 2)]);
    let vista =
        eval_ref_script(&host, r#"table("t").where("a", row.a)"#, Env::new(), &row).unwrap();
    assert_eq!(ids(&vista), ["r2"]);
}
