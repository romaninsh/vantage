//! Change-feed tests against a live `cardroom`. Ignored by default.
//!
//! `cardroom` deals itself hands continuously (players who do not act are folded
//! on a timer), so these need no client driving the game — the module is its own
//! source of change.

use std::time::Duration;

use futures::StreamExt;
use vantage_spacetimedb::SpacetimeDb;
use vantage_vista::VistaChange;

fn db() -> SpacetimeDb {
    SpacetimeDb::new(
        std::env::var("SPACETIMEDB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into()),
        std::env::var("SPACETIMEDB_DB").unwrap_or_else(|_| "cardroom".into()),
    )
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published and playing"]
async fn the_initial_subscription_delivers_the_current_set() {
    let mut factory = db().vista_factory();
    let vista = factory.from_relation("account").await.expect("build vista");
    assert!(vista.can_watch(), "spacetimedb advertises push");

    let mut stream = vista.watch().await.expect("subscribe");

    // The first frame is the subscription's initial state: every matching row,
    // delivered as inserts. A consumer can seed a cache from it directly.
    let first = tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("initial rows should arrive promptly")
        .expect("stream should not end immediately")
        .expect("and should not error");

    match first {
        VistaChange::Inserted { id, value } => {
            assert!(!id.is_empty(), "every pushed row carries an id");
            assert!(value.get("handle").is_some(), "and the whole row");
        }
        other => panic!("expected the initial state as an insert, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published and playing"]
async fn changes_arrive_without_polling() {
    // `game_event` grows every time a hand progresses, so a live game produces
    // inserts on its own.
    let mut factory = db().vista_factory();
    let vista = factory
        .from_relation("game_event")
        .await
        .expect("build vista");
    let mut stream = vista.watch().await.expect("subscribe");

    // Drain the initial state, then wait for something genuinely new.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    let mut seen_initial = false;
    let mut live_change = None;

    while tokio::time::Instant::now() < deadline {
        let Ok(Some(change)) = tokio::time::timeout_at(deadline, stream.next()).await else {
            break;
        };
        let change = change.expect("no stream errors");
        if !seen_initial {
            // The initial batch arrives first; anything after it is a live push.
            seen_initial = true;
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }
        live_change = Some(change);
        break;
    }

    let change = live_change.expect(
        "a live cardroom deals continuously, so a change should arrive within 45s — \
         is the module published and is a game in progress?",
    );
    assert!(
        matches!(change, VistaChange::Inserted { .. }),
        "game_event is append-only, so changes are inserts: got {change:?}"
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published and playing"]
async fn a_filtered_subscription_delivers_only_matching_rows() {
    // Half of what makes membership correct without client-side reconciliation:
    // the vista's conditions go into the subscription itself, so nothing outside
    // the set is ever delivered. The other half — that a row changing *out of*
    // the set arrives as a delete — needs a write to force it, and so lives in
    // `write_live::a_row_leaving_a_filtered_set_arrives_as_a_delete`.
    let mut factory = db().vista_factory();
    let mut vista = factory.from_relation("game").await.expect("build vista");
    vista
        .add_condition_eq("status", ciborium::Value::Text("playing".into()))
        .expect("status is a real column");

    let mut stream = vista.watch().await.expect("subscribe");

    // Counted, because the loop below ends on timeout, stream end and stream
    // error alike: without this a subscription that failed on the first frame
    // would sail through having asserted nothing at all.
    let mut seen = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while let Ok(Some(next)) = tokio::time::timeout_at(deadline, stream.next()).await {
        // Surfaced rather than swallowed — a stream error is the thing most
        // worth failing on here.
        let change = next.expect("the change stream must not error");
        seen += 1;
        if let VistaChange::Inserted { value, .. } | VistaChange::Updated { value, .. } = &change {
            assert_eq!(
                value["status"],
                ciborium::Value::Text("playing".into()),
                "a filtered subscription must not deliver rows outside its set"
            );
        }
    }
    assert!(
        seen > 0,
        "no change arrived in 15s — the subscription delivered nothing to check"
    );
}
