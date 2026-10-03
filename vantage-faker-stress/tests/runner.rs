use std::fs;
use std::time::Duration;

use vantage_faker_stress::runner::{RunOpts, run};
use vantage_faker_stress::scenario::load;

const SCENARIO: &str = r#"
seed: 3
tables:
  row: { count: 10, columns: { note: {} } }
sims:
  tick:
    script: "loop { let id = table().insert(#{ note: \"x\" }); sleep(seconds(0.2)); table().delete(id); }"
    spawn: { burst: 5, max: 5 }
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_short_run_samples_live_sims_and_events() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("t")).unwrap();
    fs::write(root.path().join("t/scenario.yaml"), SCENARIO).unwrap();
    let scenario = load(root.path(), "t").unwrap();
    let mut seen = 0;
    let out = run(
        &scenario,
        &RunOpts {
            duration: Duration::from_secs(3),
            dio: true,
        },
        |_| seen += 1,
    )
    .await
    .unwrap();
    assert!(out.samples.len() >= 2, "{} samples", out.samples.len());
    assert_eq!(seen, out.samples.len());
    let last = out.samples.last().unwrap();
    assert_eq!(last.live, 5);
    assert!(last.spawned >= 5);
    assert!(out.samples.iter().any(|s| s.events_per_s > 0.0));
    assert!(out.warm_secs.is_none());
    assert_eq!(out.panics, 0);
}
