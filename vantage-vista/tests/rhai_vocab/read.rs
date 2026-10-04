use serde_json::json;
use vantage_vista::Terminals;

use super::support::{host, json, read_write, run, store};

fn capped(limit: usize) -> vantage_rhai::Host {
    host(&store(), Terminals::Read { limit: Some(limit) })
}

#[test]
fn list_returns_rows_with_ids() {
    let rows = json(&read_write(&store()), r#"table("t").list()"#);
    assert_eq!(rows[0], json!({"id": "r1", "a": 1, "n": 3}));
    assert_eq!(rows.as_array().unwrap().len(), 3);
}

#[test]
fn list_is_capped_by_host_limit() {
    let rows = json(&capped(2), r#"table("t").list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 2);
}

#[test]
fn limit_below_cap_applies() {
    let rows = json(&capped(2), r#"table("t").limit(1).list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 1);
}

#[test]
fn get_missing_is_unit() {
    let host = read_write(&store());
    assert!(run(&host, r#"table("t").get("nope")"#).unwrap().is_unit());
    assert_eq!(json(&host, r#"table("t").get("r2")"#)["a"], json!(2));
}

#[test]
fn first_follows_sort() {
    let row = json(&read_write(&store()), r#"table("t").sort("n").first()"#);
    assert_eq!(row["id"], json!("r2"));
}

#[test]
fn ids_in_sort_order() {
    let host = read_write(&store());
    assert_eq!(
        json(&host, r#"table("t").ids()"#),
        json!(["r1", "r2", "r3"])
    );
    assert_eq!(
        json(&host, r#"table("t").sort("n", "desc").ids()"#),
        json!(["r1", "r3", "r2"])
    );
}

#[test]
fn count_respects_limit() {
    let host = read_write(&store());
    assert_eq!(json(&host, r#"table("t").limit(2).count()"#), json!(2));
    assert_eq!(json(&host, r#"table("t").limit(9).count()"#), json!(3));
}

#[test]
fn count_requires_capability() {
    let host = read_write(&store());
    assert_eq!(json(&host, r#"table("t").where("a", 1).count()"#), json!(2));
    let err = run(&host, r#"table("nocount").count()"#).unwrap_err();
    assert!(
        err.contains("`count` isn't supported by table `nocount`"),
        "{err}"
    );
}
