use serde_json::json;

use vantage_vista::rhai::{Terminals, Writes};

use super::support::{host, json, read_write, run, store};

fn capped(limit: usize) -> vantage_rhai::Host {
    host(
        &store(),
        Terminals::ReadWrite {
            limit: Some(limit),
            writes: Writes::Allowed,
        },
    )
}

#[test]
fn a_host_limit_caps_every_list_and_a_smaller_script_limit_applies() {
    let host = capped(2);
    let rows = json(&host, r#"table("t").list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 2);
    let rows = json(&host, r#"table("t").limit(1).list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 1);
    let rows = json(&host, r#"table("t").limit(9).list()"#);
    assert_eq!(
        rows.as_array().unwrap().len(),
        2,
        "the host cap still holds"
    );
}

#[test]
fn an_uncapped_list_returns_every_row_with_its_id() {
    let rows = json(&read_write(&store()), r#"table("t").list()"#);
    assert_eq!(rows.as_array().unwrap().len(), 3);
    assert_eq!(rows[0]["id"], json!("r1"));
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

/// The fixture holds no text cells, so any search matches nothing.
#[test]
fn search_narrows() {
    let host = read_write(&store());
    assert_eq!(json(&host, r#"table("t").search("zz").count()"#), json!(0));
}
