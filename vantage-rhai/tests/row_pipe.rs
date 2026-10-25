use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use vantage_rhai::rhai::{Dynamic, Map};
use vantage_rhai::{Host, Limits, RhaiError, RowAnswer, RowPipe};

fn row(pairs: &[(&str, Dynamic)]) -> Map {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), v.clone()))
        .collect()
}

fn pipe(script: &str) -> Arc<RowPipe> {
    Arc::new(RowPipe::spawn(Host::builder(Limits::background()), script).expect("compiles"))
}

/// `process` on a helper thread, failing the test instead of hanging it.
fn process_within(pipe: &Arc<RowPipe>, rows: Vec<Map>) -> Vec<RowAnswer> {
    let (tx, rx) = std::sync::mpsc::channel();
    let pipe = pipe.clone();
    std::thread::spawn(move || tx.send(pipe.process(rows)).unwrap());
    rx.recv_timeout(Duration::from_secs(10))
        .expect("process returned")
}

fn field(answer: &RowAnswer, key: &str) -> i64 {
    answer.as_ref().unwrap()[key].as_int().unwrap()
}

const DOUBLE: &str = r#"
    loop {
        let r = next_row();
        if r == () { break; }
        emit(#{ twice: r.n * 2 });
    }
"#;

#[test]
fn answers_come_back_in_row_order() {
    let p = pipe(DOUBLE);
    let rows = (1..=5_i64).map(|n| row(&[("n", n.into())])).collect();
    let out = process_within(&p, rows);
    let twice: Vec<i64> = out.iter().map(|a| field(a, "twice")).collect();
    assert_eq!(twice, [2, 4, 6, 8, 10]);
}

#[test]
fn an_empty_batch_is_answered_with_no_rows() {
    let p = pipe(DOUBLE);
    assert!(p.process(Vec::new()).is_empty());
}

#[test]
fn the_script_keeps_its_state_across_rows_and_batches() {
    let p = pipe(
        r#"
        let seen = 0;
        loop { let r = next_row(); if r == () { break; } seen += 1; emit(#{ seen: seen }); }
    "#,
    );
    let first = process_within(&p, vec![Map::new(), Map::new()]);
    assert_eq!(field(&first[1], "seen"), 2);
    let second = process_within(&p, vec![Map::new()]);
    assert_eq!(field(&second[0], "seen"), 3);
}

#[test]
fn setup_code_before_the_loop_runs_once() {
    let p = pipe(
        r#"
        let zones = [120, 140, 155];
        loop {
            let row = next_row();
            if row == () { break; }
            emit(#{ zone: zones.filter(|z| row.hr >= z).len() });
        }
    "#,
    );
    let out = process_within(
        &p,
        vec![
            row(&[("hr", 100_i64.into())]),
            row(&[("hr", 130_i64.into())]),
            row(&[("hr", 160_i64.into())]),
        ],
    );
    let zones: Vec<i64> = out.iter().map(|a| field(a, "zone")).collect();
    assert_eq!(zones, [0, 1, 3]);
}

#[test]
fn a_row_the_script_moves_past_without_emit_answers_with_an_empty_map() {
    let p = pipe(
        r#"
        loop {
            let r = next_row();
            if r == () { break; }
            if r.skip != true { emit(#{ ok: true }); }
        }
    "#,
    );
    let out = process_within(&p, vec![row(&[("skip", true.into())]), Map::new()]);
    assert!(out[0].as_ref().unwrap().is_empty());
    assert!(out[1].as_ref().unwrap().contains_key("ok"));
}

#[test]
fn a_throw_fails_that_row_and_the_script_restarts_from_the_top() {
    let p = pipe(
        r#"
        let n = 0;
        loop {
            let r = next_row();
            if r == () { break; }
            n += 1;
            if r.boom == true { throw "boom"; }
            emit(#{ n: n });
        }
    "#,
    );
    let out = process_within(
        &p,
        vec![Map::new(), row(&[("boom", true.into())]), Map::new()],
    );
    assert_eq!(field(&out[0], "n"), 1);
    assert!(out[1].as_ref().unwrap_err().contains("boom"));
    assert_eq!(field(&out[2], "n"), 1, "setup ran again after the throw");
}

#[test]
fn a_runtime_error_in_one_row_fails_only_that_row() {
    let p = pipe(DOUBLE);
    let out = process_within(&p, vec![Map::new(), row(&[("n", 5_i64.into())])]);
    assert!(out[0].is_err());
    assert_eq!(field(&out[1], "twice"), 10);
}

#[test]
fn a_script_that_never_asks_for_rows_errors_every_row_instead_of_hanging() {
    let p = pipe("let x = 1;");
    let out = process_within(&p, vec![Map::new(), Map::new()]);
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|a| a.is_err()));
    let again = process_within(&p, vec![Map::new()]);
    assert!(again[0].is_err(), "later batches fail too");
}

#[test]
fn a_script_that_throws_before_asking_for_rows_errors_every_row() {
    let p = pipe(r#"throw "bad setup";"#);
    let out = process_within(&p, vec![Map::new()]);
    assert!(out[0].as_ref().unwrap_err().contains("bad setup"));
}

#[test]
fn the_operation_allowance_applies_per_row_not_over_the_lifetime() {
    let builder = Host::builder(Limits::Background {
        max_operations: 20_000,
    });
    let p = Arc::new(
        RowPipe::spawn(
            builder,
            r#"
            loop {
                let r = next_row();
                if r == () { break; }
                let i = 0;
                while i < r.spin { i += 1; }
                emit(#{ done: true });
            }
        "#,
        )
        .unwrap(),
    );
    // Each row costs on the order of a thousand operations: fine alone, far
    // over the ceiling when summed across fifty rows.
    let rows = (0..50).map(|_| row(&[("spin", 200_i64.into())])).collect();
    assert!(process_within(&p, rows).iter().all(|a| a.is_ok()));
}

#[test]
fn a_runaway_row_is_stopped_and_the_next_row_is_still_served() {
    let builder = Host::builder(Limits::Background {
        max_operations: 20_000,
    });
    let p = Arc::new(
        RowPipe::spawn(
            builder,
            r#"
            loop {
                let r = next_row();
                if r == () { break; }
                let i = 0;
                while i < r.spin { i += 1; }
                emit(#{ done: true });
            }
        "#,
        )
        .unwrap(),
    );
    let out = process_within(
        &p,
        vec![
            row(&[("spin", 1_000_000_i64.into())]),
            row(&[("spin", 1_i64.into())]),
        ],
    );
    assert!(out[0].is_err(), "the runaway row fails");
    assert!(out[1].is_ok(), "the next row is still served");
}

#[test]
fn a_syntax_error_in_the_script_fails_spawn() {
    let err = RowPipe::spawn(Host::builder(Limits::Ui), "let = ;")
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(err, RhaiError::Syntax(_)), "{err:?}");
}

#[test]
fn the_builders_vocabulary_is_available_to_the_script() {
    let builder = Host::builder(Limits::background()).vocab_fn(|e| {
        e.register_fn("double", |x: i64| x * 2);
    });
    let p = Arc::new(
        RowPipe::spawn(
            builder,
            r#"loop { let r = next_row(); if r == () { break; } emit(#{ v: double(r.n) }); }"#,
        )
        .unwrap(),
    );
    let out = process_within(&p, vec![row(&[("n", 4_i64.into())])]);
    assert_eq!(field(&out[0], "v"), 8);
}

#[test]
fn dropping_the_pipe_makes_next_row_return_unit_and_ends_the_script() {
    let ended = Arc::new(AtomicBool::new(false));
    let flag = ended.clone();
    let builder = Host::builder(Limits::background()).vocab_fn(move |e| {
        let flag = flag.clone();
        e.register_fn("bye", move || flag.store(true, Ordering::SeqCst));
    });
    let p = RowPipe::spawn(
        builder,
        "loop { let r = next_row(); if r == () { break; } emit(#{}); } bye();",
    )
    .unwrap();
    drop(p);
    for _ in 0..100 {
        if ended.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("script did not see the shutdown");
}
