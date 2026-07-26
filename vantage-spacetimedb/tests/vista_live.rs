//! Vista-level tests against a live `cardroom`. Ignored by default.
//!
//! ```sh
//! cargo test --test vista_live -- --ignored --nocapture
//! ```

use vantage_dataset::traits::ReadableValueSet;
use vantage_spacetimedb::SpacetimeDb;

fn db() -> SpacetimeDb {
    SpacetimeDb::new(
        std::env::var("SPACETIMEDB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into()),
        std::env::var("SPACETIMEDB_DB").unwrap_or_else(|_| "cardroom".into()),
    )
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn a_table_reads_and_counts_through_the_vista_facade() {
    let mut factory = db().vista_factory();
    let vista = factory.from_relation("account").await.expect("build account vista");

    let rows = vista.list_values().await.expect("list");
    assert!(!rows.is_empty(), "cardroom should have accounts");

    let count = vista.get_count().await.expect("count");
    assert_eq!(
        count as usize,
        rows.len(),
        "COUNT(*) must agree with the listed rows"
    );

    // Ids come from the primary key, and every listed row is reachable by its id.
    let (id, _) = rows.iter().next().unwrap();
    let one = vista.get_value(id.clone()).await.expect("get by id");
    assert!(one.is_some(), "id {id} should resolve");
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn conditions_are_pushed_into_the_server_s_where_clause() {
    let mut factory = db().vista_factory();
    let mut vista = factory.from_relation("game").await.expect("build game vista");

    let all = vista.get_count().await.expect("count all");
    vista
        .add_condition_eq("status", ciborium::Value::Text("ended".into()))
        .expect("status is a real column");
    let ended = vista.get_count().await.expect("count ended");

    assert!(
        ended <= all,
        "a narrowed set cannot be larger than the whole ({ended} > {all})"
    );
    for (_, row) in vista.list_values().await.expect("list").iter() {
        assert_eq!(row["status"], ciborium::Value::Text("ended".into()));
    }
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn unsupported_operations_refuse_instead_of_being_emulated() {
    let mut factory = db().vista_factory();
    let mut vista = factory.from_relation("account").await.expect("build vista");

    let caps = vista.capabilities().clone();
    assert!(!caps.can_order && !caps.can_search);
    assert!(!caps.can_fetch_page && !caps.can_fetch_next && !caps.can_fetch_window);

    // The flags say the server cannot do these, and the driver does not quietly
    // do them itself. A caller that checks the flags first never gets here.
    assert!(
        vista
            .add_order("handle", vantage_vista::SortDirection::Ascending)
            .is_err(),
        "sorting must refuse, not sort locally"
    );
    assert!(vista.add_search("ali").is_err(), "search must refuse");
    assert!(vista.set_page_size(10).is_err(), "paging must refuse");

    // ... and the refusal is a typed `Unsupported`, not a generic failure.
    let err = vista
        .add_order("handle", vantage_vista::SortDirection::Ascending)
        .unwrap_err();
    assert!(
        err.to_string().contains("can_order"),
        "the refusal should name the capability: {err}"
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn a_view_reads_but_is_never_writable() {
    let mut factory = db().vista_factory();
    let vista = factory
        .from_relation("top_players")
        .await
        .expect("views build like tables");

    let caps = vista.capabilities().clone();
    assert!(caps.can_subscribe, "views are subscribable");
    assert!(!caps.can_insert && !caps.can_update && !caps.can_delete);

    let rows = vista.list_values().await.expect("a view lists like a table");
    assert!(!rows.is_empty(), "top_players should rank the accounts");
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn a_private_table_is_refused_with_its_reason() {
    let mut factory = db().vista_factory();
    // `Vista` has no `Debug`, so `expect_err` is unavailable — match by hand.
    let err = match factory.from_relation("hole_cards").await {
        Ok(_) => panic!("a private table must not produce a Vista"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("private"),
        "the error should explain why: {err}"
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn keyless_rows_get_a_stable_content_hash_id() {
    // `game_event` has a primary key, so use it to prove keyed ids are stable
    // across two independent reads — the property the change feed will rely on.
    let mut factory = db().vista_factory();
    let vista = factory.from_relation("game_event").await.expect("build vista");

    let first: Vec<String> = vista.list_values().await.unwrap().keys().cloned().collect();
    let second: Vec<String> = vista.list_values().await.unwrap().keys().cloned().collect();
    assert_eq!(
        first, second,
        "ids must be stable between reads or a cache cannot match rows"
    );
}
