//! Chapter "The table handle", plus the guide's opening example.

use serde_json::json;

use super::support::{json, read_write, run, shop};

const FIRST_SCRIPT: &str = r#"
let vips = table("client").where("vip", true);
let due = vips.ref("orders").where("status", "due");

#{
    vips: vips.count(),
    due: due.ids(),
}
"#;

#[test]
fn first_script() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, FIRST_SCRIPT),
        json!({"vips": 2, "due": ["o2", "o4"]})
    );
}

const NARROWING: &str = r#"
let paid = table("order").where("status", "paid");
let big = paid.where("total", ">", 100);
let top = paid.sort("total", "desc").first();

#{
    paid: paid.count(),
    big: big.ids(),
    top: top.id,
}
"#;

#[test]
fn narrowing() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, NARROWING),
        json!({"paid": 2, "big": ["o1"], "top": "o1"})
    );
}

const READS: &str = r#"
let clients = table("client");
let ada = clients.get("c1");
let ghost = clients.get("c9");

#{
    name: ada.name,
    missing: ghost == (),
    first_two: clients.sort("name").limit(2).list().map(|c| c.name),
}
"#;

#[test]
fn reads() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, READS),
        json!({"name": "Ada", "missing": true, "first_two": ["Ada", "Ben"]})
    );
}

const REF_SETS: &str = r#"
// The orders of one client...
let ada_orders = table("client").where("id", "c1").ref("orders");
// ...and the clients behind every due order.
let owing = table("order").where("status", "due").ref("client");

#{
    ada: ada_orders.ids(),
    owing: owing.sort("name").list().map(|c| c.name),
}
"#;

#[test]
fn ref_sets() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, REF_SETS),
        json!({"ada": ["o1", "o2"], "owing": ["Ada", "Cy"]})
    );
}

const INTROSPECTION: &str = r#"
let orders = table("order");

#{
    can_insert: orders.capabilities().can_insert,
    columns: orders.columns().map(|c| c.name),
    references: orders.references().map(|r| r.name + " " + r.kind),
}
"#;

#[test]
fn introspection() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, INTROSPECTION),
        json!({
            "can_insert": true,
            "columns": ["id", "client", "total", "status"],
            "references": ["client HasOne"],
        })
    );
}

const LATE_ERROR: &str = r#"
let invoices = table("client").ref("invoices");   // nothing resolves yet
invoices.count()                                  // throws here
"#;

#[test]
fn late_error() {
    let host = read_write(&shop());
    let err = run(&host, LATE_ERROR).unwrap_err();
    assert!(err.contains("no reference named \"invoices\""), "{err}");
}
