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

#[test]
fn ref_script_builds_target_from_row() {
    let store = store();
    let host = host(&store, Terminals::Describe);
    let row = rec(&[("a", 2)]);
    let vista =
        eval_ref_script(&host, r#"table("t").where("a", row.a)"#, Env::new(), &row).unwrap();
    assert_eq!(ids(&vista), ["r2"]);
}
