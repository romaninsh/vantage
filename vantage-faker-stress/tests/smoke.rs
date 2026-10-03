//! Every scenario starts, samples and stops. Chaos scenarios need their
//! full duration for a verdict, so that test is ignored by default:
//! `cargo test --test smoke -- --ignored`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use vantage_faker_stress::runner::{RunOpts, run};
use vantage_faker_stress::scenario::load;
use vantage_faker_stress::verdict::{describe, verdict};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("scenario.yaml").exists())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn every_load_scenario_runs_briefly() {
    let names = names(&root());
    assert!(names.len() >= 7, "{names:?}");
    for name in names {
        let s = load(&root(), &name)
            .unwrap()
            .scaled(0.1, 0.1)
            .without_warm();
        let out = run(
            &s,
            &RunOpts {
                duration: Duration::from_secs(2),
                dio: false,
            },
            |_| {},
        )
        .await
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!out.samples.is_empty(), "{name}: no samples");
        assert_eq!(out.panics, 0, "{name}: panicked");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs each chaos scenario for its full duration"]
async fn every_chaos_scenario_is_contained() {
    let chaos = root().join("chaos");
    let mut failed = Vec::new();
    for name in names(&chaos) {
        let s = load(&root(), &format!("chaos/{name}")).unwrap();
        let expect = s
            .stress
            .expect
            .clone()
            .expect("chaos scenarios set stress.expect");
        let out = run(
            &s,
            &RunOpts {
                duration: s.duration().unwrap(),
                dio: false,
            },
            |_| {},
        )
        .await
        .unwrap();
        let result = verdict(&expect, &out);
        println!("chaos/{name}: {}", describe(&result));
        if result.is_err() {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "not contained: {failed:?}");
}
