//! Chapter "Computed columns": `lazy:` columns over a memory store built from
//! YAML specs.

use std::sync::Arc;

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_dataset::ReadableValueSet;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::rhai::{block_on, lazy_value_closure};
use vantage_vista::{
    AggregateSpec, Column, DataVocab, TargetResolver, VistaFactory, VistaMetadata, Writes, flags,
};

use super::support::run;

const CLIENT: &str = r#"
name: client
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
  vip: { type: bool }
  badge:
    type: string
    lazy: |
      if row.vip { "★ " + row.name } else { row.name }
references:
  orders: { table: order, kind: has_many, foreign_key: client }
"#;

const ORDER: &str = r#"
name: order
columns:
  id: { type: string, flags: [id] }
  code: { type: string }
  net: { type: int }
  client:
    type: string
    references: client
    lazy: |
      let parts = row.code.split("-");   // "INV-c1-0001"
      parts[1]
  vat: { type: int, lazy: "row.net / 5" }
  gross: { type: int, lazy: "row.net + row.vat" }
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

/// Clients c1 Ada (vip), c2 Ben. Orders o1 INV-c1-0001 net 100, o2
/// INV-c1-0002 net 40, o3 INV-c2-0003 net 75.
fn invoices() -> (MemoryStore, TargetResolver) {
    let store = MemoryStore::new();
    let factory = Arc::new(MemoryVistaFactory::new(store.clone()));
    // Build both once, so each one's metadata is known to `ref` traversal.
    factory.from_yaml(CLIENT).unwrap();
    factory.from_yaml(ORDER).unwrap();
    for (id, name, vip) in [("c1", "Ada", true), ("c2", "Ben", false)] {
        let row: Record<CborValue> = [
            ("name".to_string(), text(name)),
            ("vip".to_string(), CborValue::Bool(vip)),
        ]
        .into_iter()
        .collect();
        store.table("client").insert_as(id, row).unwrap();
    }
    for (id, code, net) in [
        ("o1", "INV-c1-0001", 100),
        ("o2", "INV-c1-0002", 40),
        ("o3", "INV-c2-0003", 75),
    ] {
        let row: Record<CborValue> = [
            ("code".to_string(), text(code)),
            ("net".to_string(), int(net)),
        ]
        .into_iter()
        .collect();
        store.table("order").insert_as(id, row).unwrap();
    }
    let resolver: TargetResolver = Arc::new(move |name: &str| match name {
        "client" => factory.from_yaml(CLIENT),
        "order" => factory.from_yaml(ORDER),
        other => Err(vantage_core::error!("No such table", table = other)),
    });
    (store, resolver)
}

fn invoice_host() -> (MemoryStore, vantage_rhai::Host) {
    let (store, resolver) = invoices();
    let host = vantage_rhai::Host::builder(vantage_rhai::Limits::background())
        .vocab(DataVocab::read_write_with(
            Some(resolver),
            None,
            Writes::Allowed,
        ))
        .build();
    (store, host)
}

fn json(host: &vantage_rhai::Host, script: &str) -> serde_json::Value {
    vantage_rhai::to_json(&run(host, script).unwrap())
}

const READS: &str = r#"
let o1 = table("order").get("o1");

#{
    o1: [o1.client, o1.vat, o1.gross],
    badges: table("client").list().map(|c| c.badge),
    computed: table("order").columns()
        .filter(|c| c.flags.contains("calculated"))
        .map(|c| c.name),
}
"#;

#[test]
fn reads() {
    let (_store, host) = invoice_host();
    assert_eq!(
        json(&host, READS),
        json!({
            "o1": ["c1", 20, 120],
            "badges": ["★ Ada", "Ben"],
            "computed": ["client", "vat", "gross"],
        })
    );
}

#[test]
fn declare_in_rust() {
    let gross = Column::new("gross", "int")
        .with_flag("orderable")
        .with_expression("row.net + row.vat")
        .unwrap();

    assert!(gross.is_computed());
    assert_eq!(gross.expression(), Some("row.net + row.vat"));
    assert!(gross.has_flag(flags::CALCULATED));
    assert!(!gross.has_flag(flags::ORDERABLE));

    let broken = Column::new("vat", "int").with_expression("row.net /");
    assert!(broken.is_err());

    // Computed columns are part of the metadata a shell reports.
    let metadata = VistaMetadata::new()
        .with_column(Column::new("net", "int"))
        .with_column(
            Column::new("vat", "int")
                .with_expression("row.net / 5")
                .unwrap(),
        )
        .with_column(gross);
    let store = MemoryStore::new();
    let shell = vantage_memory::MemoryTableShell::new(
        store.table("order"),
        metadata.with_id_column("id"),
        vantage_memory::vista::Catalog::new(store.clone()),
    );
    let vista = vantage_vista::Vista::new("order", Box::new(shell));
    store
        .table("order")
        .insert_as("o1", [("net".to_string(), int(100))].into_iter().collect())
        .unwrap();
    let row = block_on(vista.get_value("o1".to_string()))
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(row.get("gross"), Some(&int(120)));
}

#[test]
fn declare_in_yaml() {
    let store = MemoryStore::new();
    let factory = MemoryVistaFactory::new(store);
    let orders = factory.from_yaml(ORDER).unwrap();
    let client = orders.get_column("client").unwrap();
    assert!(client.is_computed());
    assert!(client.expression().unwrap().contains("row.code.split"));
    assert_eq!(
        orders.get_reference("client").unwrap().foreign_key,
        "client"
    );

    let broken = ORDER.replace("row.net / 5", "row.net /");
    let Err(err) = factory.from_yaml(&broken) else {
        panic!("a script that doesn't parse must fail the build");
    };
    assert!(err.to_string().contains("vat"), "{err}");
}

const WRITES: &str = r#"
let orders = table("order");
orders.insert(#{ id: "o4", code: "INV-c2-0004", net: 50, gross: 1 });
orders.patch("o1", #{ net: 200, vat: 0 });

[orders.get("o4").gross, orders.get("o1").vat]
"#;

#[test]
fn writes() {
    let (store, host) = invoice_host();
    assert_eq!(json(&host, WRITES), json!([60, 40]));
    let stored = store.table("order").get("o4").unwrap();
    assert!(!stored.contains_key("gross"), "{stored:?}");
    assert!(!store.table("order").get("o1").unwrap().contains_key("vat"));
}

const WHERE_REFUSED: &str = r#"
table("order").where("gross", ">", 100).count()   // throws
"#;

const SORT_REFUSED: &str = r#"
table("order").sort("gross", "desc").list()   // throws
"#;

#[test]
fn refused() {
    let (_store, host) = invoice_host();
    for script in [WHERE_REFUSED, SORT_REFUSED] {
        let err = run(&host, script).unwrap_err();
        assert!(err.contains("Computed column can't be used"), "{err}");
        assert!(err.contains("gross"), "{err}");
    }
}

#[test]
fn aggregate_refused() {
    let (_store, resolver) = invoices();
    let orders = resolver("order").unwrap();

    let sum = orders.aggregate(&AggregateSpec::new("sum", "total").column("gross"));
    assert!(sum.is_err());
    let by_client = orders.aggregate(&AggregateSpec::new("count", "n").group_by("client"));
    assert!(by_client.is_err());

    let err = sum.err().unwrap();
    assert!(err.to_string().contains("gross"), "{err}");
}

const RELATIONS: &str = r#"
let big = table("order").where("net", ">", 50);

#{
    one: table("order").where("id", "o3").ref("client").first().name,
    many: big.ref("client").sort("name").list().map(|c| c.name),
}
"#;

const HAS_MANY_REFUSED: &str = r#"
table("client").where("id", "c1").ref("orders").ids()   // throws
"#;

#[test]
fn relations() {
    let (_store, host) = invoice_host();
    assert_eq!(
        json(&host, RELATIONS),
        json!({"one": "Ben", "many": ["Ada", "Ben"]})
    );
    let err = run(&host, HAS_MANY_REFUSED).unwrap_err();
    assert!(
        err.contains("Has-many relation can't join on a computed column"),
        "{err}"
    );
    assert!(err.contains("client"), "{err}");
}

const CLOSURE: &str = r#"
row.contents.split("\n").len() - 1
"#;

#[test]
fn closure() {
    let compute = lazy_value_closure(CLOSURE).unwrap();
    let row: Record<CborValue> = [("contents".to_string(), text("a\nb\nc\n"))]
        .into_iter()
        .collect();
    assert_eq!(compute(&row).unwrap(), int(3));
}
