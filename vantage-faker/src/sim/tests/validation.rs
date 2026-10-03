//! `SimDef::validate` and the engine-level config checks.

use super::*;

fn ok() -> SimDef {
    SimDef::new("s", "log", "sleep(seconds(1));")
}

fn err(def: SimDef) -> String {
    def.validate().unwrap_err().to_string()
}

#[test]
fn a_plain_def_is_valid() {
    assert!(ok().validate().is_ok());
}

#[test]
fn bad_defs_are_rejected_with_a_clear_message() {
    let cases: Vec<(SimDef, &str, &str)> = vec![
        (
            SimDef {
                script: "  ".into(),
                ..ok()
            },
            "Sim script is empty",
            "",
        ),
        (
            SimDef {
                table: "".into(),
                ..ok()
            },
            "Sim table is empty",
            "",
        ),
        (ok().with_spawn(0, 0.0, 0), "Sim max must be at least 1", ""),
        (
            ok().with_spawn(5, 0.0, 2),
            "Sim burst is above its max",
            "burst: 5",
        ),
        (
            ok().with_spawn(1, -1.0, 1),
            "Sim rate must be finite",
            "rate: -1.0",
        ),
        (
            ok().with_spawn(1, f64::NAN, 1),
            "Sim rate must be finite",
            "rate: NaN",
        ),
        (
            ok().with_clock(0.0),
            "Sim clock must be finite and positive",
            "clock: 0.0",
        ),
        (ok().with_clock(-2.0), "Sim clock must be", "clock: -2.0"),
        (
            ok().with_clock(f64::INFINITY),
            "Sim clock must be",
            "clock: inf",
        ),
        (ok().with_ops(0), "Sim ops must be greater than 0", ""),
        (
            ok().with_spawn(1, 0.0, MAX_LIVE + 1),
            "Sim max is above the live-sim limit",
            "max: 1001",
        ),
        (
            SimDef {
                script: "let x = ;".into(),
                ..ok()
            },
            "Sim script does not compile",
            "",
        ),
    ];
    for (def, want, value) in cases {
        let got = err(def);
        assert!(got.contains(want), "{got:?} lacks {want:?}");
        assert!(got.contains(value), "{got:?} lacks {value:?}");
        assert!(got.contains("sim: \"s\""), "{got:?}");
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
    assert!(e.contains("Sim table does not exist"), "{e}");
    assert!(e.contains("table: \"elsewhere\""), "{e}");

    let big = |name: &str| SimDef::new(name, "log", "1").with_spawn(0, 0.0, 600);
    let e = start_err(vec![big("a"), big("b")]);
    assert!(
        e.contains("max adds up to") && e.contains("total: 1200"),
        "{e}"
    );

    let e = start_err(vec![ok().with_clock(0.0)]);
    assert!(e.contains("clock: 0.0"), "{e}");
}
