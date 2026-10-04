//! Chapter "Writes".

use serde_json::json;

use super::support::{host, json, read_write, run, shop};
use vantage_vista::{Terminals, Writes};

const WRITE_VERBS: &str = r#"
let orders = table("order");

let id = orders.insert(#{ client: "c2", total: 30, status: "due" });
orders.patch(id, #{ status: "paid" });

let deleted = orders.delete("o4");   // true: the row was there
let again = orders.delete("o4");     // false: nothing left to delete
let patched = orders.patch("o99", #{ status: "paid" });   // false

#{
    status: orders.get(id).status,
    deleted: deleted,
    again: again,
    patched: patched,
}
"#;

#[test]
fn write_verbs() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, WRITE_VERBS),
        json!({"status": "paid", "deleted": true, "again": false, "patched": false})
    );
}

const EXPLICIT_IDS: &str = r#"
let orders = table("order");

orders.insert(#{ id: "o10", client: "c3", total: 55, status: "due" });
orders.upsert("o10", #{ client: "c3", total: 60, status: "due" });   // replaces
orders.upsert("o11", #{ client: "c3", total: 5, status: "due" });    // inserts

table("client").where("id", "c3").ref("orders").ids()
"#;

#[test]
fn explicit_ids() {
    let host = read_write(&shop());
    assert_eq!(json(&host, EXPLICIT_IDS), json!(["o4", "o10", "o11"]));
    assert_eq!(json(&host, r#"table("order").get("o10").total"#), json!(60));
}

const DUPLICATE_INSERT: &str = r#"
table("order").insert(#{ id: "o1", client: "c2", total: 1, status: "due" })
"#;

#[test]
fn duplicate_insert() {
    let host = read_write(&shop());
    assert!(run(&host, DUPLICATE_INSERT).is_err());
}

const WRITES_IGNORE_NARROWING: &str = r#"
let paid = table("order").where("status", "paid");
paid.delete("o2");   // o2 is due, and is deleted all the same

let ada = table("client").where("id", "c1");
ada.ref("orders").insert(#{ id: "o20", client: "c1", total: 9, status: "due" });

#{
    orders: table("order").ids(),
    clients: table("client").count(),
}
"#;

#[test]
fn writes_ignore_narrowing() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, WRITES_IGNORE_NARROWING),
        json!({"orders": ["o1", "o3", "o4", "o20"], "clients": 3})
    );
}

const IMPORT_MAPPED: &str = r#"
let report = table("archive").import_from(
    table("order").where("status", "paid"),
    #{ id: "a-${row.id}", amount: "${row.total}", note: "paid by ${row.client}" }
);

#{ report: report, rows: table("archive").list() }
"#;

#[test]
fn import_mapped() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, IMPORT_MAPPED),
        json!({
            "report": {"inserted": 2, "skipped": 0, "cancelled": false},
            "rows": [
                {"id": "a-o1", "amount": 120, "note": "paid by c1"},
                {"id": "a-o3", "amount": 75, "note": "paid by c2"},
            ],
        })
    );
}

const IMPORT_TWICE: &str = r#"
let first = table("backup").import_from(table("client"));
let second = table("backup").import_from(table("client"));

[first.inserted, second.inserted, second.skipped]
"#;

#[test]
fn import_twice() {
    let host = read_write(&shop());
    assert_eq!(json(&host, IMPORT_TWICE), json!([3, 0, 3]));
}

const IMPORT_LIMITED: &str = r#"
let biggest = table("order").sort("total", "desc").limit(2);
let report = table("archive").import_from(biggest);

[report.inserted, table("archive").ids()]
"#;

#[test]
fn import_limited() {
    let host = read_write(&shop());
    assert_eq!(json(&host, IMPORT_LIMITED), json!([2, ["o1", "o3"]]));
}

#[test]
fn denied_writes() {
    let store = shop();
    let host = host(
        &store,
        Terminals::ReadWrite {
            limit: None,
            writes: Writes::Denied("writes are off for agents".into()),
        },
    );
    let err = run(&host, r#"table("order").delete("o1")"#).unwrap_err();
    assert!(err.contains("writes are off for agents"), "{err}");
    assert_eq!(json(&host, r#"table("order").count()"#), json!(4));
}
