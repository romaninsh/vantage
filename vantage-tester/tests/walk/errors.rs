use vantage_tester::{Outcome, Report, ScenarioReport, Status};

use super::map_host::MapHost;
use super::{failure, not_run, run, scenario};

fn outcomes(s: &ScenarioReport) -> Vec<&Outcome> {
    s.steps.iter().map(|st| &st.outcome).collect()
}

#[tokio::test]
async fn later_what_ifs_of_a_red_step_are_reported_not_run() {
    let yaml = r#"
scenarios:
  main:
    steps:
      - { step: a, do: insert order a, what_if: [x] }
      - { step: b, do: insert order b, assert: [count order == 9] }
      - { step: c, do: insert order c, what_if: [y] }
  x:
    steps: [{ step: x1, do: insert order x }]
  y:
    steps:
      - { step: y1 }
      - { step: y2, do: insert order y2, what_if: [z] }
  z:
    steps: [{ step: z1 }]
"#;
    let report = run(&MapHost::default(), yaml).await;
    let labels: Vec<_> = report.scenarios.iter().map(ScenarioReport::label).collect();
    assert_eq!(labels, ["main", "x ← main", "y ← main", "z ← y ← main"]);
    assert_eq!(
        scenario(&report, "main").steps[2].outcome,
        not_run("an earlier step failed")
    );
    assert_eq!(outcomes(scenario(&report, "x ← main")), [&Outcome::Passed]);
    let parent = not_run("parent step failed");
    assert_eq!(outcomes(scenario(&report, "y ← main")), [&parent, &parent]);
    assert_eq!(outcomes(scenario(&report, "z ← y ← main")), [&parent]);
    let junit = Report {
        files: vec![report],
    }
    .to_junit();
    assert_eq!(junit.matches("<testcase ").count(), 7, "{junit}");
}

#[tokio::test]
async fn a_load_error_is_one_red_error() {
    let yaml =
        "fixtures: broken\nscenarios:\n  main:\n    steps: [{ step: a, do: insert order a }]\n";
    let host = MapHost::default();
    let report = run(&host, yaml).await;
    assert_eq!(report.status, Status::Red);
    assert!(report.error.as_deref().unwrap().contains("broken"));
    assert!(report.scenarios.is_empty());
    assert_eq!(host.runs_of("a"), 0);
    let junit = Report {
        files: vec![report],
    }
    .to_junit();
    assert!(junit.contains(r#"tests="1" failures="1""#), "{junit}");
    assert!(junit.contains(r#"name="(load)""#), "{junit}");
}

#[tokio::test]
async fn a_fork_error_fails_the_what_if() {
    let yaml = r#"
scenarios:
  main:
    steps:
      - { step: a, do: insert nofork n }
      - { step: b, do: insert order b, what_if: [x] }
      - { step: c, do: insert order c, fails: "refused" }
  x:
    steps:
      - { step: x1 }
      - { step: x2, what_if: [w] }
  w:
    steps: [{ step: w1 }]
"#;
    let report = run(&MapHost::default(), yaml).await;
    assert_eq!(report.status, Status::Red);
    assert!(failure(&report, "main", 2).starts_with("fork:"));
    let x = scenario(&report, "x ← main");
    assert!(x.error.as_deref().unwrap().starts_with("fork:"), "{x:#?}");
    let unstarted = not_run("the scenario didn't start");
    assert_eq!(outcomes(x), [&unstarted, &unstarted]);
    assert_eq!(
        outcomes(scenario(&report, "w ← x ← main")),
        [&not_run("parent step failed")]
    );
}
