//! Chapter "Surfaces": the runners and script slots that host the vocabulary.

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::rhai::{Dynamic, EvalAltResult};
use vantage_rhai::{Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::rhai::block_on;
use vantage_vista::{
    DataVocab, FilterOp, Handle, Terminals, Vista, Writes, augment_source_closure, dynamic_to_cbor,
    eval_modify_script, eval_ref_script, preview_script, run_script,
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

const EXTENSION_STATEMENTS: &str = r#"
self.only("vip", true);
self.only("name", "Cy");
"#;

const MIXED_STATEMENTS: &str = r#"
self.only("vip", true);
self.sort("name", "desc");
"#;

/// A stand-in for a backend extension verb: `only(col, value)` narrows the
/// Vista in hand by equality.
fn only(h: &mut Handle, col: &str, value: Dynamic) -> Result<Handle, Box<EvalAltResult>> {
    let (col, value) = (col.to_string(), dynamic_to_cbor(value)?);
    h.with_base_vista("only", |vista| {
        vista.add_condition(col, FilterOp::Eq, value)
    })
    .map_err(|e| e.to_string().into())
}

#[test]
fn extension_statements() {
    let store = shop();
    let host = Host::builder(Limits::background())
        .vocab_fn(|engine| {
            engine.register_fn("only", only);
        })
        .vocab(DataVocab::describe(Some(resolver(&store))))
        .build();
    let base = || resolver(&store)("client").unwrap();

    let vista = eval_modify_script(&host, EXTENSION_STATEMENTS, base()).unwrap();
    assert_eq!(ids(&vista), ["c3"]);

    // A stored handle keeps what it was given.
    let kept = "let all = self; all.only(\"vip\", true); all";
    assert_eq!(
        ids(&eval_modify_script(&host, kept, base()).unwrap()),
        ["c1", "c2", "c3"]
    );

    // Narrowing statements accumulate with extension statements.
    let vista = eval_modify_script(&host, MIXED_STATEMENTS, base()).unwrap();
    assert_eq!(ids(&vista), ["c3", "c1"]);

    // Narrowing chains on an extension verb's result.
    let chained = r#"self.only("vip", true).sort("name", "desc")"#;
    assert_eq!(
        ids(&eval_modify_script(&host, chained, base()).unwrap()),
        ["c3", "c1"]
    );

    // Outside `self`, an extension verb is an error naming it.
    let err = run(&host, r#"table("client").only("vip", true)"#).unwrap_err();
    assert!(err.contains("not on a handle from `table(...)`"), "{err}");
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
