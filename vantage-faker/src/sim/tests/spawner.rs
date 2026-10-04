//! Spawners (burst, rate, max), the `spawn` verb and the engine cap.

use super::*;

#[test]
fn burst_then_rate_over_a_simulated_minute() {
    let def = SimDef::new(
        "s",
        "log",
        r#"table().insert(#{ who: "s", at: now_secs() }); sleep(hours(1));"#,
    )
    .with_spawn(3, 6.0, 100);
    let (engine, log) = engine_with(vec![def]);
    engine.settle();
    assert_eq!(rows(&log).len(), 3, "the burst starts at once");

    run_for(&engine, 60, 1);
    let at: Vec<f64> = rows(&log).iter().map(|r| num(r, "at")).collect();
    assert_eq!(at.len(), 9, "six more over the minute: {at:?}");
    let t0 = T0 as f64;
    assert_eq!(
        &at[3..],
        [10.0, 20.0, 30.0, 40.0, 50.0, 60.0].map(|s| t0 + s)
    );
}

#[test]
fn rate_follows_the_sim_clock() {
    let def = SimDef::new(
        "s",
        "log",
        r#"table().insert(#{ who: "s" }); sleep(hours(1));"#,
    )
    .with_spawn(0, 1.0, 100)
    .with_clock(60.0);
    let (engine, log) = engine_with(vec![def]);
    // One per sim minute at 60x is one per real second.
    run_for(&engine, 10, 1);
    assert_eq!(rows(&log).len(), 10);
}

#[test]
fn max_caps_concurrent_sims() {
    let def = SimDef::new(
        "s",
        "log",
        r#"table().insert(#{ who: "s" }); sleep(seconds(10));"#,
    )
    .with_spawn(0, 60.0, 3);
    let (engine, log) = engine_with(vec![def]);
    let mut peak = 0;
    for _ in 0..40 {
        engine.advance(Duration::from_secs(1));
        engine.settle();
        peak = peak.max(engine.live_of("s"));
    }
    assert_eq!(peak, 3);
    // A sim ends every 10 s and frees one slot, taken on the next tick.
    let started = rows(&log).len();
    assert!((10..=15).contains(&started), "{started} started");
}

#[test]
fn spawn_verb_respects_the_child_max_and_passes_args() {
    let parent = SimDef::new(
        "parent",
        "log",
        r#"
            let got = [];
            for i in 0..4 { got.push(spawn_sim("child", #{ n: i })); }
            table().insert(#{ who: "parent", step: got.filter(|x| x).len() });
        "#,
    );
    let child = SimDef::new(
        "child",
        "log",
        r#"table().insert(#{ who: "child", step: args.n }); sleep(hours(1));"#,
    )
    .with_spawn(0, 0.0, 2);
    let (engine, log) = engine_with(vec![parent, child]);
    engine.settle();
    let all = rows(&log);
    let parent_row = all.iter().find(|r| text(r, "who") == "parent").unwrap();
    assert_eq!(num(parent_row, "step"), 2.0);
    let mut kids: Vec<f64> = all
        .iter()
        .filter(|r| text(r, "who") == "child")
        .map(|r| num(r, "step"))
        .collect();
    kids.sort_by(f64::total_cmp);
    assert_eq!(kids, [0.0, 1.0]);
    assert_eq!(engine.live_of("child"), 2);
}

#[test]
fn spawner_args_are_in_scope() {
    let def = SimDef::new(
        "a",
        "log",
        r#"table().insert(#{ who: args.name, step: args.n });"#,
    )
    .with_args(serde_json::json!({ "name": "argued", "n": 7 }));
    let (engine, log) = engine_with(vec![def]);
    engine.settle();
    let row = &rows(&log)[0];
    assert_eq!(text(row, "who"), "argued");
    assert_eq!(num(row, "step"), 7.0);
}

#[test]
fn an_engine_runs_up_to_max_live_sims() {
    let def = SimDef::new("crowd", "log", "sleep(hours(1));").with_spawn(MAX_LIVE, 0.0, MAX_LIVE);
    let (engine, _log) = engine_with(vec![def]);
    engine.settle();
    assert_eq!(engine.live(), MAX_LIVE);
    assert_eq!(engine.threads(), MAX_LIVE);
    engine.stop();
    assert_eq!(engine.threads(), 0);
}

#[test]
fn engine_spawn_starts_a_sim_with_args_and_respects_max() {
    let def = SimDef::new(
        "nurse",
        "log",
        r#"table().insert(#{ who: args.name, step: 1 }); sleep(hours(1));"#,
    )
    .with_spawn(0, 0.0, 2)
    .with_args(serde_json::json!({ "name": "default" }));
    let (engine, log) = engine_with(vec![def]);
    engine.settle();
    assert!(rows(&log).is_empty(), "no burst, no rate");

    let mut args = vantage_rhai::rhai::Map::new();
    args.insert("name".into(), "ada".into());
    assert!(engine.spawn("nurse", Some(args)).unwrap());
    assert!(engine.spawn("nurse", None).unwrap());
    assert!(!engine.spawn("nurse", None).unwrap(), "at max");
    engine.settle();

    let mut who: Vec<String> = rows(&log).iter().map(|r| text(r, "who")).collect();
    who.sort();
    assert_eq!(who, ["ada", "default"]);
    assert_eq!(engine.live_of("nurse"), 2);
}

#[test]
fn engine_spawn_rejects_an_unknown_name_and_a_stopped_engine() {
    let def = SimDef::new("nurse", "log", "sleep(hours(1));").with_spawn(0, 0.0, 5);
    let (engine, _log) = engine_with(vec![def]);
    let err = engine.spawn("doctor", None).unwrap_err().to_string();
    assert!(err.contains("doctor"), "{err}");
    engine.stop();
    assert!(!engine.spawn("nurse", None).unwrap());
}
