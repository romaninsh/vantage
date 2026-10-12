//! Chapter "Surfaces": the runners and script slots that host the vocabulary.

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_dataset::ReadableValueSet;
use vantage_rhai::rhai::{Dynamic, EvalAltResult};
use vantage_rhai::{Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::rhai::{
    DataVocab, Handle, Terminals, Writes, block_on, dynamic_to_cbor, eval_modify_script,
    eval_ref_script, preview_script, run_script,
};
use vantage_vista::{Condition, FilterOp, NativeCondition, TableShell, Vista, forward_table_shell};

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
    assert_eq!(
        out["query"],
        json!(r#"scan order WHERE status = "paid" ORDER BY total DESC"#)
    );
}

const MODIFY: &str = r#"
self.where("vip", true).sort("name", "desc")
"#;

#[test]
fn modify() {
    let store = shop();
    let host = host(&store, Terminals::Describe);
    let base = resolver(&store).vista("client").unwrap();
    let vista = eval_modify_script(&host, MODIFY, base).unwrap();
    assert_eq!(ids(&vista), ["c3", "c1"]);
}

const EXTENSION_CHAIN: &str = r#"
self.with_condition("vip", true).with_condition("name", "Cy")
"#;

const MIXED_CHAIN: &str = r#"
self.with_condition("vip", true).sort("name", "desc")
"#;

const KEPT: &str = r#"
let all = self;
let vip = all.with_condition("vip", true);
all
"#;

const DROPPED_STATEMENT: &str = r#"
self.with_condition("vip", true);    // throws: this handle would be dropped
self.sort("name", "desc");
"#;

const DROPPED_ALIAS: &str = r#"
let all = self;
all.with_condition("vip", true);    // throws: this handle would be dropped
all
"#;

const NOT_A_HANDLE: &str = r#"
let vip_only = false;
if vip_only {
    self.with_condition("vip", true)
}   // throws: a false `if` with no `else { self }` is ()
"#;

/// A stand-in for a backend's `with_condition` verb: an equality passed to
/// the shell as a native condition.
fn with_condition(h: &mut Handle, col: &str, value: Dynamic) -> Result<Handle, Box<EvalAltResult>> {
    let key = format!("{col} = {value}");
    let condition = Condition::new(col, FilterOp::Eq, dynamic_to_cbor(value)?);
    h.with_native_condition("with_condition", NativeCondition::new(condition, key))
        .map_err(|e| e.to_string().into())
}

/// The shell side of the stand-in: a native condition is a [`Condition`]
/// the inner shell applies.
#[derive(Clone)]
struct NativeEq {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(NativeEq, inner, {
    fn add_native_condition(&mut self, condition: NativeCondition) -> vantage_core::Result<()> {
        let condition: Condition = condition
            .downcast()
            .map_err(|_| vantage_core::error!("not a Condition"))?;
        self.inner.add_condition(&condition)
    }
});

#[test]
fn extension_chains() {
    let store = shop();
    let host = Host::builder(Limits::background())
        .vocab_fn(|engine| {
            engine.register_fn("with_condition", with_condition);
        })
        .vocab(DataVocab::new(Terminals::Describe).with_resolver(resolver(&store)))
        .build();
    let base = || {
        resolver(&store)
            .vista("client")
            .unwrap()
            .map_shell(|inner| NativeEq { inner })
    };

    let vista = eval_modify_script(&host, EXTENSION_CHAIN, base()).unwrap();
    assert_eq!(ids(&vista), ["c3"]);

    // A stored handle keeps what it was given.
    let vista = eval_modify_script(&host, KEPT, base()).unwrap();
    assert_eq!(ids(&vista), ["c1", "c2", "c3"]);

    // Narrowing chains on an extension verb's result.
    let vista = eval_modify_script(&host, MIXED_CHAIN, base()).unwrap();
    assert_eq!(ids(&vista), ["c3", "c1"]);

    // A statement that builds a handle and drops it is refused, whatever
    // handle it starts from.
    for script in [DROPPED_STATEMENT, DROPPED_ALIAS] {
        let err = eval_modify_script(&host, script, base()).unwrap_err();
        assert!(
            err.to_string().contains("table handle is discarded"),
            "{err}"
        );
    }

    // The script's value is its last statement, and it must be a table handle.
    let err = eval_modify_script(&host, NOT_A_HANDLE, base()).unwrap_err();
    assert!(
        err.to_string().contains("must end on a table handle"),
        "{err}"
    );

    // Outside `self`, an extension verb is an error naming it.
    let err = run(&host, r#"table("client").with_condition("vip", true)"#).unwrap_err();
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

#[test]
fn describe_host_has_no_terminals() {
    let store = shop();
    let host = host(&store, Terminals::Describe);
    assert!(run(&host, r#"table("order").count()"#).is_err());
}

const READ_HOST: &str = r#"
let o = table("order").record("o1");
o.status = "void";
o.flash()   // throws: this host only reads
"#;

#[test]
fn read_host() {
    let store = shop();
    let terminals = Terminals::ReadWrite {
        limit: Some(50),
        writes: Writes::Denied("writes aren't available here".into()),
    };
    let host = host(&store, terminals);
    for script in [READ_HOST, r#"table("order").delete("o1")"#] {
        let err = run(&host, script).unwrap_err();
        assert!(err.contains("writes aren't available here"), "{err}");
    }
}
