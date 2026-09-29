//! Linear scripts: state in locals, sleeps, `done()`, errors and stopping.

use std::time::Instant;

use super::*;

#[test]
fn linear_script_keeps_its_locals_across_sleeps() {
    let script = r#"
        let n = 0;
        let id = insert(#{ who: "a", step: n, at: now_secs() });
        while n < 3 {
            sleep(seconds(10));
            n += 1;
            patch(id, #{ step: n, at: now_secs() });
        }
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script)]);
    engine.settle();
    assert_eq!(rows(&log).len(), 1);
    assert_eq!(num(&rows(&log)[0], "step"), 0.0);

    run_for(&engine, 25, 5);
    let row = &rows(&log)[0];
    assert_eq!(num(row, "step"), 2.0);
    assert_eq!(num(row, "at"), (T0 + 20) as f64, "the sim clock is exact");

    run_for(&engine, 10, 5);
    assert_eq!(num(&rows(&log)[0], "step"), 3.0);
    assert_eq!(engine.live(), 0, "the sim ends with its script");
    assert_eq!(engine.threads(), 0);
}

#[test]
fn sleepers_wake_in_time_order_across_sims() {
    let sleeper = |name: &str, secs: u32| {
        SimDef::new(
            name,
            "log",
            format!(r#"sleep(seconds({secs})); insert(#{{ who: "{name}", at: now_secs() }});"#),
        )
    };
    let (engine, log) = engine_with(vec![
        sleeper("slow", 30),
        sleeper("fast", 10),
        sleeper("mid", 20),
    ]);
    run_for(&engine, 40, 1);
    let order: Vec<String> = rows(&log).iter().map(|r| text(r, "who")).collect();
    assert_eq!(order, ["fast", "mid", "slow"]);
}

#[test]
fn done_ends_the_sim_early_and_keeps_its_rows() {
    let script = r#"
        insert(#{ who: "kept" });
        done();
        insert(#{ who: "never" });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("d", "log", script)]);
    engine.settle();
    let who: Vec<String> = rows(&log).iter().map(|r| text(r, "who")).collect();
    assert_eq!(who, ["kept"]);
    assert_eq!(engine.live(), 0);
}

#[test]
fn done_cannot_be_caught() {
    let script = r#"
        try { done(); } catch { }
        insert(#{ who: "escaped" });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("d", "log", script)]);
    engine.settle();
    assert!(rows(&log).is_empty());
}

#[test]
fn a_failing_sim_ends_without_stopping_the_others() {
    let bad =
        SimDef::new("bad", "log", r#"sleep(seconds(1)); throw "boom";"#).with_spawn(3, 0.0, 3);
    let good = SimDef::new(
        "good",
        "log",
        r#"sleep(seconds(5)); insert(#{ who: "good" });"#,
    );
    let (engine, log) = engine_with(vec![bad, good]);
    run_for(&engine, 2, 1);
    assert_eq!(engine.live_of("bad"), 0);
    assert_eq!(engine.live_of("good"), 1);
    run_for(&engine, 5, 1);
    assert_eq!(rows(&log).len(), 1);
}

#[test]
fn deep_recursion_fails_the_sim_within_its_small_stack() {
    let script = r#"
        fn deep(n) { if n == 0 { 0 } else { 1 + deep(n - 1) } }
        insert(#{ who: "before" });
        deep(10000);
        insert(#{ who: "after" });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("r", "log", script)]);
    engine.settle();
    let who: Vec<String> = rows(&log).iter().map(|r| text(r, "who")).collect();
    assert_eq!(who, ["before"]);
    assert_eq!(engine.threads(), 0);
}

#[test]
fn unknown_table_is_a_script_error() {
    let script = r#"insert("nope", #{ who: "x" }); insert(#{ who: "unreached" });"#;
    let (engine, log) = engine_with(vec![SimDef::new("t", "log", script)]);
    engine.settle();
    assert!(rows(&log).is_empty());
    assert_eq!(engine.live(), 0);
}

#[test]
fn stop_ends_sleepers_promptly_on_the_system_clock() {
    let store = store_with(&["log"]);
    let log = store.table("log");
    let def = SimDef::new(
        "nap",
        "log",
        "sleep(hours(10)); insert(#{ who: \"late\" });",
    )
    .with_spawn(50, 0.0, 50);
    let engine = SimEngine::builder().store(&store).sim(def).start().unwrap();
    engine.settle();
    assert_eq!(engine.threads(), 50);
    let t = Instant::now();
    engine.stop();
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "stop took {:?}",
        t.elapsed()
    );
    assert_eq!(engine.threads(), 0);
    assert_eq!(engine.live(), 0);
    assert!(rows(&log).is_empty());
}

/// Short sleeps on a fast system clock keep every sim moving: a deadline
/// that passes between a sleeper registering and parking is due at once, not
/// an untimed park that nothing live ever wakes.
#[test]
fn short_sleeps_on_the_system_clock_never_strand_a_sim() {
    let store = store_with(&["log"]);
    let log = store.table("log");
    let def = SimDef::new(
        "tick",
        "log",
        "for i in 0..40 { sleep(seconds(1)); } insert(#{ who: \"done\" });",
    )
    .with_spawn(60, 0.0, 60)
    .with_clock(240.0);
    let engine = SimEngine::builder().store(&store).sim(def).start().unwrap();
    // 40 sim seconds at 240x is about 0.17 s real; allow plenty.
    let deadline = Instant::now() + Duration::from_secs(10);
    while rows(&log).len() < 60 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rows(&log).len(), 60, "every sim finished its sleeps");
    engine.stop();
}

#[test]
fn stop_ends_a_busy_script() {
    let (engine, _log) = engine_with(vec![SimDef::new("spin", "log", "loop { }")]);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(engine.threads(), 1);
    let t = Instant::now();
    engine.stop();
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "stop took {:?}",
        t.elapsed()
    );
    assert_eq!(engine.threads(), 0);
}

#[test]
fn dropping_the_engine_joins_its_threads() {
    let def = SimDef::new("nap", "log", "sleep(hours(1));").with_spawn(20, 0.0, 20);
    let engine = SimEngine::builder()
        .store(&store_with(&["log"]))
        .sim(def)
        .manual_clock(start())
        .start()
        .unwrap();
    engine.settle();
    let live = Arc::downgrade(&engine.inner_for_tests());
    drop(engine);
    assert_eq!(
        live.strong_count(),
        0,
        "no sim thread still holds the engine"
    );
}

#[test]
fn the_deepest_allowed_nesting_fits_the_stack() {
    let levels = SIM_CALL_LEVELS - 1;
    let script = |parens: usize| {
        let nested = format!("{}1{}", "(".repeat(parens), ")".repeat(parens));
        format!(
            r#"
            fn deep(n) {{ if n == 0 {{ {nested} }} else {{ 1 + deep(n - 1) }} }}
            insert(#{{ who: "r", step: deep({levels}) }});
            "#
        )
    };
    let def = |parens| SimDef::new("r", "log", script(parens));
    let parens = (1..SIM_EXPR_DEPTHS.1)
        .rev()
        .find(|p| def(*p).validate().is_ok())
        .unwrap();
    let (engine, log) = engine_with(vec![def(parens)]);
    engine.settle();
    assert_eq!(num(&rows(&log)[0], "step"), (levels + 1) as f64);
}
