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
    let vista = factory
        .from_relation("account")
        .await
        .expect("build account vista");

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
    let mut vista = factory
        .from_relation("game")
        .await
        .expect("build game vista");

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

    // ... and the refusal is a typed `Unsupported`, not a generic failure —
    // which is what lets a caller distinguish "this backend cannot" from "this
    // request failed" and fall back deliberately.
    //
    // The refusal arrives from the column flag rather than from `can_order`,
    // because no column of this driver is flagged ORDERABLE and `Vista` checks
    // that before reaching the shell. Two honest refusals of the same fact; the
    // test asserts the fact, not which guard got there first.
    let err = vista
        .add_order("handle", vantage_vista::SortDirection::Ascending)
        .unwrap_err();
    assert!(
        err.is_unsupported(),
        "the refusal must be a typed Unsupported: {err}"
    );
    assert!(
        err.to_string().contains("orderable"),
        "the refusal should say it is about ordering: {err}"
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

    let rows = vista
        .list_values()
        .await
        .expect("a view lists like a table");
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
async fn ids_are_stable_across_reads() {
    // The property the change feed rests on: a row read twice answers to the
    // same id both times, so a pushed delete can be matched against a cached
    // insert. `game_event` is the natural subject — it is append-only, so a row
    // that appears in both reads must be byte-identical in both.
    //
    // What this deliberately does *not* do is compare the two id *sets*.
    // `cardroom` is a live database: hands are being dealt while the test runs,
    // so the second read legitimately holds events the first could not have
    // seen. Asserting set equality tests the dealer's idleness, not the driver.
    let mut factory = db().vista_factory();
    let vista = factory
        .from_relation("game_event")
        .await
        .expect("build vista");

    let first = vista.list_values().await.unwrap();
    let second = vista.list_values().await.unwrap();

    let mut compared = 0;
    for (id, row) in first.iter() {
        let Some(again) = second.get(id) else {
            continue; // legitimately gone only if the log were trimmed
        };
        assert_eq!(
            row, again,
            "id {id} named different rows in two reads — a cache would mismatch them"
        );
        compared += 1;
    }
    assert!(
        compared > 0,
        "no row survived between reads, so nothing was actually compared"
    );
}
