//! A pass keeps the order of its changes: on the cache, on the bus, and
//! around an `Invalidated` or a re-read.

use super::*;

/// A pass leaves the cache and the bus as one-at-a-time application would:
/// a later row for an id wins, and a delete lands after the upserts before it.
#[tokio::test]
async fn a_pass_applies_in_order() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        VistaChange::Updated {
            id: "a".into(),
            value: row("2"),
        },
        inserted("b", "1"),
        VistaChange::Deleted { id: "b".into() },
        inserted("c", "1"),
    ];
    apply_all(&dio, &Admit::All, changes).await.unwrap();

    let cache = &dio.inner.cache;
    assert_eq!(cache.get_value("a").await.unwrap(), Some(row("2")));
    assert_eq!(cache.get_value("b").await.unwrap(), None);
    assert_eq!(cache.get_value("c").await.unwrap(), Some(row("1")));
    assert_eq!(
        drain(&mut bus),
        [
            "inserted a",
            "changed a",
            "inserted b",
            "removed b",
            "inserted c"
        ]
    );
}

/// `Invalidated` flushes the run before it and keeps its place in the order.
#[tokio::test]
async fn invalidated_flushes_the_run_first() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        VistaChange::Invalidated,
        inserted("b", "1"),
        inserted("c", "1"),
    ];
    apply_all(&dio, &Admit::All, changes).await.unwrap();
    assert_eq!(
        drain(&mut bus),
        [
            "inserted a",
            "refreshing",
            "dataset changed",
            "inserted b",
            "inserted c"
        ]
    );
}

/// Changes the follow can't judge cost one re-read for the whole pass, made
/// once the ready changes are in.
#[tokio::test]
async fn unjudgeable_changes_cost_one_reread_after_the_pass() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        VistaChange::Deleted { id: "c".into() },
        inserted("b", "1"),
    ];
    apply_all(&dio, &Admit::Reread, changes).await.unwrap();
    assert_eq!(drain(&mut bus), ["refreshing", "dataset changed"]);
}
