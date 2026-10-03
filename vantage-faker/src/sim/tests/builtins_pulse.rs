//! `builtin:pulse`: an aggregate that drifts in a band, a short-lived feed
//! of its changes and windowed per-minute buckets.

use super::*;

fn pulse_engine(args: serde_json::Value) -> (SimEngine, MemoryStore) {
    let store = store_with(&["top", "updates", "minutes"]);
    let def = SimDef::new(
        "pulse",
        "top",
        crate::sim::builtin::builtin("pulse").unwrap(),
    )
    .with_args(args);
    let engine = SimEngine::builder()
        .store(&store)
        .sim(def)
        .manual_clock(start())
        .seed(5)
        .start()
        .unwrap();
    (engine, store)
}

fn args() -> serde_json::Value {
    serde_json::json!({
        "key_column": "region", "value_column": "visitors",
        "keys": [ { "name": "North", "baseline": 1000 }, { "name": "South", "baseline": 400 } ],
        "band": 5, "feed_retention": 10, "bucket": 60, "window": 3, "offline": ["South"]
    })
}

#[test]
fn aggregate_values_stay_in_band() {
    let (engine, store) = pulse_engine(args());
    run_for(&engine, 300, 1);
    let north = store.table("top").get("North").unwrap();
    let v = num(&north, "visitors");
    assert!((950.0..=1050.0).contains(&v), "{v}");
}

#[test]
fn aggregate_values_are_integers_for_integer_baselines() {
    // A 433-style baseline with a band whose edge is fractional (±5% of 433).
    let mut a = args();
    a["keys"] = serde_json::json!([{ "name": "North", "baseline": 433 }]);
    let (engine, store) = pulse_engine(a);
    for _ in 0..300 {
        run_for(&engine, 1, 1);
        if let Some(r) = store.table("top").get("North") {
            assert!(
                matches!(r.get("visitors"), Some(CborValue::Integer(_))),
                "{:?}",
                r.get("visitors")
            );
        }
    }
}

#[test]
fn feed_rows_expire_and_buckets_are_windowed() {
    let (engine, store) = pulse_engine(args());
    run_for(&engine, 600, 1);
    assert!(
        store.table("updates").len() <= 20,
        "feed keeps ~retention worth"
    );
    assert!(store.table("minutes").len() <= 3, "window trims buckets");
    assert!(!store.table("minutes").is_empty());
}

#[test]
fn offline_keys_freeze() {
    let (engine, store) = pulse_engine(args());
    let mut saw_offline = false;
    for _ in 0..120 {
        run_for(&engine, 1, 1);
        if let Some(r) = store.table("top").get("South")
            && text(&r, "live") == "Offline"
        {
            saw_offline = true;
        }
    }
    assert!(saw_offline);
}
