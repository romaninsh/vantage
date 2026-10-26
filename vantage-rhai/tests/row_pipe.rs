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
fn rows_are_answered_in_order_by_one_script_that_keeps_its_state() {
    let builder = Host::builder(Limits::background()).vocab_fn(|e| {
        e.register_fn("double", |x: i64| x * 2);
    });
    let p = Arc::new(
        RowPipe::spawn(
            builder,
            r#"
            let seen = 0;
            loop {
                let r = next_row();
                if r == () { break; }
                seen += 1;
                if r.skip != true { emit(#{ seen: seen, twice: double(r.n) }); }
            }
        "#,
        )
        .unwrap(),
    );
    let first = process_within(
        &p,
        vec![
            row(&[("n", 1_i64.into())]),
            row(&[("n", 2_i64.into())]),
            row(&[("skip", true.into())]),
        ],
    );
    let second = process_within(&p, vec![row(&[("n", 3_i64.into())])]);
    assert_eq!(field(&first[0], "twice"), 2);
    assert_eq!(field(&first[1], "twice"), 4);
    assert!(first[2].as_ref().unwrap().is_empty());
    assert_eq!(field(&second[0], "seen"), 4);
    assert_eq!(field(&second[0], "twice"), 6);
}

#[test]
fn an_empty_batch_is_answered_with_no_rows() {
    let p = pipe(DOUBLE);
    assert!(p.process(Vec::new()).is_empty());
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
fn the_operation_allowance_is_per_row_so_only_a_runaway_row_fails() {
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
    // Each small row costs on the order of a thousand operations: fine alone, far
    // over the ceiling when summed across fifty rows.
    let mut rows = vec![row(&[("spin", 200_i64.into())]); 50];
    rows.push(row(&[("spin", 1_000_000_i64.into())]));
    rows.push(row(&[("spin", 1_i64.into())]));
    let out = process_within(&p, rows);
    assert!(
        out[..50].iter().all(|a| a.is_ok()),
        "fifty small rows pass together"
    );
    assert!(out[50].is_err(), "the runaway row fails");
    assert!(out[51].is_ok(), "the next row is still served");
}

#[test]
fn a_syntax_error_in_the_script_fails_spawn() {
    let err = RowPipe::spawn(Host::builder(Limits::Ui), "let = ;")
        .map(|_| ())
        .unwrap_err();
    assert!(matches!(err, RhaiError::Syntax(_)), "{err:?}");
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
