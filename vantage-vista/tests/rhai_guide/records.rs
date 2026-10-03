//! Chapter "Records".

use serde_json::json;

use super::support::{json, read_write, run, shop};

const EDIT_EXISTING: &str = r#"
let o = table("order").record("o2");
o.status = "paid";
o["total"] = 45;

let was = o.baseline().status;
let staged = [o.dirty("status"), o.dirty("client")];
o.save();   // patches `status` and `total`, nothing else

#{
    was: was,
    staged: staged,
    dirty: o.is_dirty(),
    stored: table("order").get("o2"),
}
"#;

#[test]
fn edit_existing() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, EDIT_EXISTING),
        json!({
            "was": "due",
            "staged": [true, false],
            "dirty": false,
            "stored": {"id": "o2", "client": "c1", "total": 45, "status": "paid"},
        })
    );
}

const NEW_ROW: &str = r#"
let n = table("client").record();
n.set(#{ name: "Dee", vip: false });
let id = n.save();   // an insert; the backend picks the id

#{
    same_id: n.id == id,
    status: n.status(),
    stored: table("client").get(id).name,
}
"#;

#[test]
fn new_row() {
    let host = read_write(&shop());
    assert_eq!(
        json(&host, NEW_ROW),
        json!({"same_id": true, "status": "tracking", "stored": "Dee"})
    );
}

const REVERT: &str = r#"
let c = table("client").record("c2");
c.name = "Benjamin";
c.vip = true;

c.revert("vip");
let still_dirty = c.is_dirty();   // `name` is still staged
c.revert();

[still_dirty, c.is_dirty(), c.name]
"#;

#[test]
fn revert() {
    let host = read_write(&shop());
    assert_eq!(json(&host, REVERT), json!([true, false, "Ben"]));
}

const FAILED_SAVE: &str = r#"
let o = table("order").record("o3");
table("order").delete("o3");   // the row goes away underneath the draft

o.status = "refunded";
let threw = false;
try { o.save(); } catch (err) { threw = true; }

#{
    threw: threw,
    status: o.status(),
    message: o.rejection().message,
    staged: o.dirty("status"),
}
"#;

#[test]
fn failed_save() {
    let host = read_write(&shop());
    let out = json(&host, FAILED_SAVE);
    assert_eq!(out["threw"], json!(true));
    assert_eq!(out["status"], json!("failed"));
    assert_eq!(out["staged"], json!(true));
    let message = out["message"].as_str().unwrap();
    assert!(message.contains("no longer exists"), "{message}");
}

const ID_IS_READ_ONLY: &str = r#"
let n = table("client").record();
n.set(#{ id: "c9", name: "Eve" })   // throws: `id` is the id column
"#;

#[test]
fn id_is_read_only() {
    let host = read_write(&shop());
    let err = run(&host, ID_IS_READ_ONLY).unwrap_err();
    assert!(err.contains("id column"), "{err}");
}
