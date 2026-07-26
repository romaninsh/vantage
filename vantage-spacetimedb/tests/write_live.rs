//! Write-path tests against a live `cardroom`. Ignored by default.
//!
//! Writes need an owner token; set `SPACETIMEDB_TOKEN`, or mint one:
//!
//! ```sh
//! export SPACETIMEDB_TOKEN=$(curl -s -X POST http://127.0.0.1:3000/v1/identity | jq -r .token)
//! cargo test --test write_live -- --ignored
//! ```

use vantage_dataset::traits::ReadableValueSet;
use vantage_spacetimedb::SpacetimeDb;

fn url() -> String {
    std::env::var("SPACETIMEDB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into())
}

fn database() -> String {
    std::env::var("SPACETIMEDB_DB").unwrap_or_else(|_| "cardroom".into())
}

fn authed() -> SpacetimeDb {
    let token = std::env::var("SPACETIMEDB_TOKEN")
        .expect("set SPACETIMEDB_TOKEN — writes are refused for anonymous callers");
    SpacetimeDb::new(url(), database()).with_token(token)
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published"]
async fn anonymous_writes_are_refused_before_a_request_is_sent() {
    // The host would refuse this anyway, but with a permission error that says
    // nothing about the cause. Catching it here names the fix.
    let mut factory = SpacetimeDb::new(url(), database()).vista_factory();
    let vista = factory.from_relation("config").await.expect("build vista");

    let caps = vista.capabilities().clone();
    assert!(
        !caps.can_insert && !caps.can_update && !caps.can_delete,
        "an anonymous connection must not advertise writes"
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published"]
async fn a_token_turns_the_write_capabilities_on() {
    let mut factory = authed().vista_factory();
    let vista = factory.from_relation("config").await.expect("build vista");

    let caps = vista.capabilities().clone();
    assert!(caps.can_insert && caps.can_update && caps.can_delete);
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published"]
async fn a_view_stays_read_only_even_with_a_token() {
    let mut factory = authed().vista_factory();
    let vista = factory
        .from_relation("top_players")
        .await
        .expect("build view vista");

    let caps = vista.capabilities().clone();
    assert!(
        !caps.can_insert && !caps.can_update && !caps.can_delete,
        "a view has nothing to write to, token or not"
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published"]
async fn a_row_can_be_updated_and_read_back() {
    use vantage_dataset::traits::WritableValueSet;

    // `config` is a public singleton, so it is safe to nudge and restore.
    let mut factory = authed().vista_factory();
    let vista = factory.from_relation("config").await.expect("build vista");

    let rows = vista.list_values().await.expect("list config");
    let (id, before) = rows.iter().next().expect("cardroom seeds a config row");
    let original = before["turn_timeout_secs"].clone();

    let mut patch = vantage_types::Record::new();
    patch.insert(
        "turn_timeout_secs".into(),
        ciborium::Value::Integer(21.into()),
    );
    let after = vista
        .patch_value(id.clone(), &patch)
        .await
        .expect("patch should apply");

    assert_eq!(
        after["turn_timeout_secs"],
        ciborium::Value::Integer(21.into()),
        "the updated value should be read back, not the old one"
    );

    // Restore, so a re-run starts from the same state.
    let mut restore = vantage_types::Record::new();
    restore.insert("turn_timeout_secs".into(), original.clone());
    vista
        .patch_value(id.clone(), &restore)
        .await
        .expect("restore");
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published"]
async fn a_write_is_visible_on_the_change_feed() {
    use futures::StreamExt;
    use std::time::Duration;
    use vantage_dataset::traits::WritableValueSet;
    use vantage_vista::VistaChange;

    // The two halves of the driver should agree: a write through the SQL path
    // must surface on the subscription path.
    let mut factory = authed().vista_factory();
    let vista = factory.from_relation("config").await.expect("build vista");
    let mut stream = vista.watch().await.expect("subscribe");

    // Drain the initial state.
    let _ = tokio::time::timeout(Duration::from_secs(5), stream.next()).await;

    let rows = vista.list_values().await.expect("list");
    let (id, before) = rows.iter().next().expect("config row");
    // A column no other test touches: `config` is a singleton, so two tests
    // writing the same field would race and each could observe the other's value.
    let original = before["starting_capital"].clone();
    let marker = ciborium::Value::Integer(12_345.into());

    let mut patch = vantage_types::Record::new();
    patch.insert("starting_capital".into(), marker.clone());
    vista.patch_value(id.clone(), &patch).await.expect("patch");

    // Wait for *our* write specifically: the feed may carry another test's
    // change to the same row first.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut saw_our_update = false;
    while let Ok(Some(Ok(change))) = tokio::time::timeout_at(deadline, stream.next()).await {
        // A changed row is a delete + insert of the same key in one transaction,
        // so the driver must report it as an update rather than a remove-then-add.
        if let VistaChange::Updated { value, .. } = &change {
            if value["starting_capital"] == marker {
                saw_our_update = true;
                break;
            }
        } else if let VistaChange::Deleted { .. } = &change {
            panic!("a delete+insert pair must not surface as a delete: {change:?}");
        }
    }
    assert!(
        saw_our_update,
        "the write should have arrived on the change feed as an Updated"
    );

    let mut restore = vantage_types::Record::new();
    restore.insert("starting_capital".into(), original.clone());
    vista
        .patch_value(id.clone(), &restore)
        .await
        .expect("restore");
}
