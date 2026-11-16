#[path = "walk/errors.rs"]
mod errors;
#[path = "walk/map_host.rs"]
mod map_host;

use std::sync::Mutex;

use map_host::MapHost;
use vantage_tester::{Event, FileReport, Outcome, ScenarioReport, Status, parse, plan, seed, walk};

fn not_run(reason: &str) -> Outcome {
    Outcome::NotRun {
        reason: reason.into(),
    }
}

async fn run(host: &MapHost, yaml: &str) -> FileReport {
    let file = parse(yaml, "t.yaml").unwrap();
    let tree = plan(&file).unwrap();
    walk(host, &file, &tree, &|_| {}).await
}

fn scenario<'r>(report: &'r FileReport, label: &str) -> &'r ScenarioReport {
    report
        .scenarios
        .iter()
        .find(|s| s.label() == label)
        .unwrap_or_else(|| panic!("no {label}"))
}

fn failure(report: &FileReport, label: &str, step: usize) -> String {
    match &scenario(report, label).steps[step].outcome {
        Outcome::Failed { message, .. } => message.clone(),
        other => panic!("{label} step {step}: {other:?}"),
    }
}

const BRANCHES: &str = r#"
scenarios:
  main:
    steps:
      - { step: a, do: insert order a }
      - { step: b, do: insert order b, what_if: [x, y] }
      - { step: c, do: insert order c, assert: [no order x, no order y, has order b] }
  x:
    steps:
      - { step: x, do: insert order x, assert: [count order == 3, has order a, no order b, no order y] }
  y:
    steps:
      - { step: y, do: insert order y, assert: [no order x, no order b] }
"#;

#[tokio::test]
async fn steps_above_a_what_if_run_once() {
    let host = MapHost::default();
    let events = Mutex::new(Vec::new());
    let file = parse(BRANCHES, "t.yaml").unwrap();
    let report = walk(&host, &file, &plan(&file).unwrap(), &|e| {
        events.lock().unwrap().push(e)
    })
    .await;
    assert_eq!(report.status, Status::Green, "{report:#?}");
    let runs: Vec<_> = ["a", "b", "c", "x", "y"]
        .iter()
        .map(|s| host.runs_of(s))
        .collect();
    assert_eq!(runs, [1, 1, 1, 1, 1]);
    assert_eq!(*host.seeds.lock().unwrap(), [seed("t.yaml", "main")]);
    let labels: Vec<_> = report.scenarios.iter().map(ScenarioReport::label).collect();
    assert_eq!(labels, ["main", "x ← main", "y ← main"]);
    let events = events.lock().unwrap();
    assert!(
        matches!(&events[0], Event::FileStarted { total: 5, .. }),
        "{:?}",
        events[0]
    );
    let started = events
        .iter()
        .filter(|e| matches!(e, Event::StepStarted { .. }))
        .count();
    assert_eq!(started, 5);
    for label in labels {
        let s = scenario(&report, &label);
        assert!(
            s.steps.iter().all(|st| st.outcome == Outcome::Passed),
            "{s:#?}"
        );
    }
    assert_eq!(scenario(&report, "x ← main").steps[0].writes[0].id, "x");
}

#[test]
fn walk_future_is_send() {
    fn assert_send<T: Send>(_: T) {}
    let host = MapHost::default();
    let file = parse(BRANCHES, "t.yaml").unwrap();
    let tree = plan(&file).unwrap();
    assert_send(walk(&host, &file, &tree, &|_| {}));
}

#[tokio::test]
async fn a_step_that_should_fail_but_passes_is_red() {
    let yaml = r#"
scenarios:
  main:
    steps:
      - { step: lenient, do: insert order z, fails: "refused" }
      - { step: after, do: insert order w }
"#;
    let report = run(&MapHost::default(), yaml).await;
    assert_eq!(report.status, Status::Red);
    assert!(failure(&report, "main", 0).contains("but the step passed"));
    assert_eq!(
        scenario(&report, "main").steps[1].outcome,
        not_run("an earlier step failed")
    );
}

#[tokio::test]
async fn every_step_finishes_after_a_failure() {
    let yaml = r#"
scenarios:
  main:
    steps:
      - { step: a, do: fail broke }
      - { step: b, do: insert order b, what_if: [x] }
  x:
    steps:
      - { step: x, do: insert order x }
      - { step: x2, do: insert order x2 }
"#;
    let file = parse(yaml, "t.yaml").unwrap();
    let tree = plan(&file).unwrap();
    let finished = Mutex::new(0);
    walk(&MapHost::default(), &file, &tree, &|e| {
        if matches!(e, Event::StepFinished { .. }) {
            *finished.lock().unwrap() += 1;
        }
    })
    .await;
    assert_eq!(*finished.lock().unwrap(), tree.step_count());
}

#[tokio::test]
async fn a_fails_mismatch_reports_both_messages() {
    let yaml = r#"
scenarios:
  kept-nothing:
    steps:
      - { step: half-way, do: insert order z; fail not enough cash, fails: "enough cash" }
      - { step: after, before: insert order w, assert: [no order z, has order w] }
  mismatch:
    steps:
      - { step: wrong, do: fail auction has ended, fails: "Not enough cash" }
  harness:
    steps:
      - { step: typo, do: bogus command, fails: "unknown command" }
"#;
    let report = run(&MapHost::default(), yaml).await;
    assert!(!scenario(&report, "kept-nothing").failed(), "{report:#?}");
    let message = failure(&report, "mismatch", 0);
    assert!(
        message.contains("Not enough cash") && message.contains("auction has ended"),
        "{message}"
    );
    let harness = failure(&report, "harness", 0);
    assert!(harness.contains("couldn't run"), "{harness}");
    assert_eq!(report.status, Status::Red);
}

#[tokio::test]
async fn an_unwritten_what_if_is_yellow_after_the_walk() {
    let yaml = "scenarios:\n  main:\n    steps:\n      - { step: a, do: insert order a, what_if: [ghost, later] }\n  later:\n    skip: needs a deadline\n";
    let report = run(&MapHost::default(), yaml).await;
    assert_eq!(report.status, Status::Yellow);
    assert_eq!(report.unwritten.len(), 1);
    assert_eq!(report.unwritten[0].name.as_deref(), Some("ghost"));
    assert_eq!(
        scenario(&report, "later ← main").skip.as_deref(),
        Some("needs a deadline")
    );

    let settled = run(&MapHost::default(), &yaml.replace("ghost, ", "")).await;
    assert_eq!(settled.status, Status::Green);
}

#[tokio::test]
async fn junit_has_one_case_per_step() {
    let yaml = r#"
scenarios:
  main:
    steps:
      - { step: a, do: insert order a, what_if: [x, ghost] }
      - { step: b, do: insert order b, assert: [count order == 9] }
      - { step: c, do: insert order c }
  x:
    steps:
      - { step: "x <&>", do: insert order x }
"#;
    let report = run(&MapHost::default(), yaml).await;
    let junit = vantage_tester::Report {
        files: vec![report],
    }
    .to_junit();
    assert_eq!(junit.matches("<testcase ").count(), 5, "{junit}");
    assert_eq!(junit.matches("<failure ").count(), 1);
    assert_eq!(junit.matches("<skipped ").count(), 2);
    assert!(
        junit.contains(
            r#"classname="main" name="what-if ghost (a)"><skipped message="not written"/>"#
        ),
        "{junit}"
    );
    assert!(
        junit.contains(r#"classname="x ← main" name="x &lt;&amp;&gt;""#),
        "{junit}"
    );
    assert!(
        junit.contains(r#"<testsuite name="t.yaml" tests="5" failures="1" skipped="2">"#),
        "{junit}"
    );
}
