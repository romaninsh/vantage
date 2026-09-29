//! `SimDef::validate` and the engine-level config checks.

use super::*;

fn ok() -> SimDef {
    SimDef::new("s", "log", "sleep(seconds(1));")
}

fn err(def: SimDef) -> String {
    def.validate().unwrap_err()
}

#[test]
fn a_plain_def_is_valid() {
    assert_eq!(ok().validate(), Ok(()));
}

#[test]
fn bad_defs_are_rejected_with_a_clear_message() {
    let cases: Vec<(SimDef, &str)> = vec![
        (
            SimDef {
                script: "  ".into(),
                ..ok()
            },
            "script is empty",
        ),
        (
            SimDef {
                table: "".into(),
                ..ok()
            },
            "table is empty",
        ),
        (ok().with_spawn(0, 0.0, 0), "max must be at least 1"),
        (ok().with_spawn(5, 0.0, 2), "burst 5 is above max 2"),
        (ok().with_spawn(1, -1.0, 1), "rate -1 must be finite"),
        (ok().with_spawn(1, f64::NAN, 1), "rate NaN must be finite"),
        (ok().with_clock(0.0), "clock 0 must be finite and positive"),
        (ok().with_clock(-2.0), "clock -2 must be"),
        (ok().with_clock(f64::INFINITY), "clock inf must be"),
        (ok().with_ops(0), "ops must be greater than 0"),
        (
            ok().with_spawn(1, 0.0, MAX_LIVE + 1),
            "max 1001 is above the limit of 1000",
        ),
        (
            SimDef {
                script: "let x = ;".into(),
                ..ok()
            },
            "script does not compile",
        ),
    ];
    for (def, want) in cases {
        let got = err(def);
        assert!(got.contains(want), "{got:?} lacks {want:?}");
        assert!(got.starts_with("sim"), "{got:?}");
    }
}

fn start_err(defs: Vec<SimDef>) -> String {
    let mut b = SimEngine::builder()
        .store(&store_with(&["log"]))
        .manual_clock(start());
    for d in defs {
        b = b.sim(d);
    }
    match b.start() {
        Ok(_) => panic!("engine started"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn engine_config_is_checked_as_a_whole() {
    let e = start_err(vec![ok(), ok()]);
    assert!(e.contains("defined twice"), "{e}");

    let e = start_err(vec![SimDef::new("s", "elsewhere", "1")]);
    assert!(e.contains("table elsewhere does not exist"), "{e}");

    let big = |name: &str| SimDef::new(name, "log", "1").with_spawn(0, 0.0, 600);
    let e = start_err(vec![big("a"), big("b")]);
    assert!(e.contains("max adds up to 1200"), "{e}");

    let e = start_err(vec![ok().with_clock(0.0)]);
    assert!(e.contains("clock 0"), "{e}");
}
