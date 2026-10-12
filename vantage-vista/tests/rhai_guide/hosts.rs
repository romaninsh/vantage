//! Chapter "Hosts": building a host around `DataVocab` and running it with
//! and without a tokio runtime.

use serde_json::json;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_vista::rhai::{DataVocab, Terminals, Writes};

use super::support::{resolver, shop};

#[test]
fn build_a_host() {
    let store = shop();
    let terminals = Terminals::ReadWrite {
        limit: Some(50),
        writes: Writes::Allowed,
    };
    let host = Host::builder(Limits::background())
        .vocab(DataVocab::new(terminals).with_resolver(resolver(&store)))
        .build();

    let script = host
        .compile(&Block::from(
            r#"table("order").where("status", "due").count()"#,
        ))
        .unwrap();
    let due = script.eval(&Env::new()).unwrap();
    assert_eq!(due.as_int().unwrap(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn under_spawn_blocking() {
    let store = shop();
    let resolver = resolver(&store);
    let rows = tokio::task::spawn_blocking(move || {
        let terminals = Terminals::ReadWrite {
            limit: Some(2),
            writes: Writes::Denied("this host only reads".into()),
        };
        let host = Host::builder(Limits::background())
            .vocab(DataVocab::new(terminals).with_resolver(resolver))
            .build();
        host.compile(&Block::from(r#"table("order").list()"#))
            .and_then(|s| s.eval(&Env::new()))
            .map(|v| vantage_rhai::to_json(&v))
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["id"], json!("o1"));
}
