//! Chapter "Surfaces": the runners and script slots that host the vocabulary.

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::Env;
use vantage_types::Record;
use vantage_vista::rhai::{block_on, lazy_value_closure};
use vantage_vista::{
    Terminals, Vista, Writes, augment_source_closure, eval_modify_script, eval_ref_script,
    preview_script, run_script,
};

use super::support::{host, resolver, run, shop};

fn ids(vista: &Vista) -> Vec<String> {
    block_on(vista.list_values())
        .unwrap()
        .unwrap()
        .into_keys()
        .collect()
}

fn client_row(id: &str) -> Record<CborValue> {
    [("id".to_string(), CborValue::Text(id.into()))]
        .into_iter()
        .collect()
}

const AGENT_SCRIPT: &str = r#"
let due = table("order").where("status", "due");
#{ count: due.count(), rows: due.sort("total", "desc").list() }
"#;

#[tokio::test(flavor = "multi_thread")]
async fn agent_script() {
    let store = shop();
    let out = run_script(
        AGENT_SCRIPT.to_string(),
        resolver(&store),
        1,
        Writes::Denied("writing data is turned off".into()),
    )
    .await
    .unwrap();
    assert_eq!(out["count"], json!(2));
    assert_eq!(
        out["rows"],
        json!([{"id": "o2", "client": "c1", "total": 40, "status": "due"}])
    );
}

const PREVIEW: &str = r#"
table("order").where("status", "paid").sort("total", "desc")
"#;

#[test]
fn preview() {
    let out = preview_script(PREVIEW.to_string(), resolver(&shop())).unwrap();
    assert_eq!(out["driver"], json!("memory"));
    assert_eq!(out["table"], json!("order"));
    assert_eq!(out["order"], json!([["total", "desc"]]));
}

const MODIFY: &str = r#"
self.where("vip", true).sort("name", "desc")
"#;

#[test]
fn modify() {
    let store = shop();
    let host = host(&store, Terminals::Describe);
    let base = resolver(&store)("client").unwrap();
    let vista = eval_modify_script(&host, MODIFY, base).unwrap();
    assert_eq!(ids(&vista), ["c3", "c1"]);
}

const REF_BUILD: &str = r#"
table("order").where("client", row.id).where("status", "due")
"#;

#[test]
fn ref_build() {
    let store = shop();
    let host = host(&store, Terminals::Describe);
    let vista = eval_ref_script(&host, REF_BUILD, Env::new(), &client_row("c1")).unwrap();
    assert_eq!(ids(&vista), ["o2"]);
}

const AUGMENT: &str = r#"
self.where("client", row.id)
"#;

#[test]
fn augment() {
    let store = shop();
    let source = augment_source_closure(resolver(&store), AUGMENT.to_string());
    let base = resolver(&store)("order").unwrap();
    let vista = source(&client_row("c1"), base).unwrap();
    assert_eq!(ids(&vista), ["o1", "o2"]);
}

#[test]
fn describe_host_has_no_terminals() {
    let store = shop();
    let host = host(&store, Terminals::Describe);
    assert!(run(&host, r#"table("order").count()"#).is_err());
}

const LAZY_COLUMN: &str = r#"
row.contents.split("\n").len() - 1
"#;

#[test]
fn lazy_column() {
    let compute = lazy_value_closure(LAZY_COLUMN).unwrap();
    let row: Record<CborValue> = [("contents".to_string(), CborValue::Text("a\nb\nc\n".into()))]
        .into_iter()
        .collect();
    assert_eq!(compute(&row).unwrap(), CborValue::Integer(3.into()));
}

const READ_HOST: &str = r#"
let o = table("order").record("o1");
o.status = "void";
o.save()   // throws: this host only reads
"#;

#[test]
fn read_host() {
    let store = shop();
    let host = host(&store, Terminals::Read { limit: Some(50) });
    let err = run(&host, READ_HOST).unwrap_err();
    assert!(err.contains("writes aren't available here"), "{err}");
    assert!(run(&host, r#"table("order").delete("o1")"#).is_err());
}
