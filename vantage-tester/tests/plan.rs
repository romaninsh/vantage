#[path = "plan/inherit.rs"]
mod inherit;

use std::path::Path;

use vantage_tester::{
    Branch, ClockOffset, ClockUnit, Do, RecordSource, ScenarioFile, Session, Tree, VarDef, WhatIf,
    load, parse, plan,
};

const FUNDING: &str = include_str!("data/funding.yaml");

fn funding() -> ScenarioFile {
    parse(FUNDING, "tests/scenarios/funding.yaml").unwrap()
}

fn plan_yaml(yaml: &str) -> vantage_core::Result<Tree> {
    plan(&parse(yaml, "t.yaml")?)
}

fn err(yaml: &str) -> String {
    plan_yaml(yaml).unwrap_err().to_string()
}

fn branch_names(tree: &Tree, root: usize, step: usize) -> Vec<String> {
    tree.roots[root].steps[step]
        .what_ifs
        .iter()
        .map(|b| match b {
            Branch::Scenario(n) => n.name.clone(),
            Branch::Missing(m) => format!("missing:{m}"),
            Branch::Unnamed => "unnamed".into(),
        })
        .collect()
}

#[test]
fn the_spec_example_parses() {
    let file = funding();
    assert_eq!(
        file.scenarios["no-cash"].steps[0].before.as_deref(),
        Some(
            "set(\"txn\", \"txn:drain\", #{ investor: carol.id, kind: \"withdrawal\", amount: -4900 })\n"
        )
    );
    assert_eq!(file.fixtures.0, vec!["acme-part-funded"]);
    assert_eq!(
        file.vars["acme"],
        VarDef::Record {
            record: "loan".into(),
            source: RecordSource::Id("loan:acme".into())
        }
    );
    let VarDef::Vista { vista, where_ } = &file.vars["acme_active_bids"] else {
        panic!()
    };
    assert_eq!(
        (vista.as_str(), where_["loan"].as_str()),
        ("bid", Some("$acme"))
    );

    let funded = &file.scenarios["funded"];
    let Some(Session::Api { app, fields }) = &funded.session else {
        panic!()
    };
    assert_eq!(
        (app.as_str(), fields["investor_id"].as_str()),
        ("portal", Some("$carol"))
    );
    assert_eq!(funded.clock.as_deref(), Some("2026-11-14T09:00:00Z"));
    let close = &funded.steps[1];
    assert_eq!(
        close.clock,
        Some(ClockOffset {
            amount: 2,
            unit: ClockUnit::Days
        })
    );
    assert_eq!(close.as_, Some(Session::Desktop));
    assert!(
        matches!(&close.do_, Some(Do::Action { action, record: Some(r), .. }) if action == "close_auction" && r == "$acme")
    );
    assert_eq!(
        funded.steps[0].what_if,
        Some(WhatIf::Named(vec![
            "no-cash".into(),
            "too-small".into(),
            "too-late".into()
        ]))
    );
    assert_eq!(
        funded.steps[0].vars["carol_bid"],
        VarDef::Record {
            record: "bid".into(),
            source: RecordSource::From("result.bid".into())
        }
    );

    let tree = plan(&file).unwrap();
    let roots: Vec<_> = tree.roots.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(roots, ["funded"]);
    assert_eq!(
        branch_names(&tree, 0, 0),
        ["no-cash", "too-small", "too-late"]
    );
    assert_eq!(branch_names(&tree, 0, 1), ["missing:revoked"]);
}

#[test]
fn unquoted_rhai_cut_at_a_hash_is_refused() {
    let yaml = "scenarios:\n  a:\n    steps:\n      - step: s\n        before: set(carol, #{ cash: 100 })\n";
    let file = parse(yaml, "t.yaml").unwrap();
    assert_eq!(
        file.scenarios["a"].steps[0].before.as_deref(),
        Some("set(carol,")
    );
    let refused = plan(&file).unwrap_err().to_string();
    assert!(
        refused.contains("looks cut short") && refused.contains("quote"),
        "{refused}"
    );
}

#[test]
fn a_scenario_file_outside_the_root_is_refused() {
    let base = std::env::temp_dir().join(format!("vantage-tester-load-{}", std::process::id()));
    let root = base.join("project");
    std::fs::create_dir_all(root.join("tests")).unwrap();
    let yaml = "scenarios:\n  a:\n    steps:\n      - step: s\n";
    std::fs::write(root.join("tests/a.yaml"), yaml).unwrap();
    std::fs::write(base.join("b.yaml"), yaml).unwrap();

    let inside = load(&root, Path::new("tests/../tests/a.yaml")).unwrap();
    assert_eq!(inside.path, "tests/a.yaml");
    assert_eq!(
        load(&root, &root.join("tests/a.yaml")).unwrap().path,
        "tests/a.yaml"
    );
    let outside = load(&root, &base.join("b.yaml")).unwrap_err().to_string();
    assert!(outside.contains("outside the project root"), "{outside}");
    assert!(load(&root, Path::new("../b.yaml")).is_err());
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn rhai_comments_are_skipped_by_the_cut_check() {
    let step = |rhai: &str| {
        let body = rhai.replace('\n', "\n          ");
        format!(
            "scenarios:\n  a:\n    steps:\n      - step: s\n        before: |\n          {body}\n"
        )
    };
    plan_yaml(&step("let x = 1; // don't (\n/* a { /* nested ' */ */ x")).unwrap();
    plan_yaml(&step("f(1) // trailing,")).unwrap();
    assert!(err(&step("f(1, // note")).contains("looks cut short"));
    assert!(err(&step("f(1) /* open")).contains("looks cut short"));
}

#[test]
fn an_unwritten_what_if_is_yellow() {
    let unwritten = plan(&funding()).unwrap().unwritten();
    let found: Vec<_> = unwritten
        .iter()
        .map(|u| (u.name.as_deref(), u.from.as_str()))
        .collect();
    assert_eq!(found, [(Some("revoked"), "funded")]);

    let tree = plan_yaml("scenarios:\n  a:\n    steps:\n      - step: s\n        what_if: true\n")
        .unwrap();
    assert_eq!(tree.unwritten()[0].name, None);
    assert_eq!(tree.unwritten()[0].step, "s");
}

#[test]
fn a_what_if_scenario_cannot_set_its_own_start() {
    let base = "scenarios:\n  a:\n    steps:\n      - step: s\n        what_if: [b]\n  b:\n";
    for own in [
        "    session: desktop\n",
        "    clock: 2026-01-01T00:00:00Z\n",
        "    fixtures: other\n",
    ] {
        let yaml = format!("{base}{own}    steps:\n      - step: t\n");
        assert!(plan_yaml(&yaml).is_err(), "accepted: {own}");
    }
    let message = err(&format!(
        "{base}    session: desktop\n    steps:\n      - step: t\n"
    ));
    assert!(message.contains("can't set `session`"), "{message}");
}

#[test]
fn a_skipped_root_with_a_what_if_plans() {
    let yaml = "scenarios:\n  a:\n    skip: not yet\n    steps: [{ step: one, do: x, what_if: [b] }]\n  b:\n    steps: [{ step: two, what_if: [c] }]\n  c:\n    steps: [{ step: three }]\n";
    let tree = plan_yaml(yaml).unwrap();
    assert_eq!(tree.roots.len(), 1);
    assert!(tree.roots[0].steps.is_empty());
    assert_eq!(tree.step_count(), 0);
}

#[test]
fn step_count_excludes_skipped_scenarios() {
    let yaml = "scenarios:\n  a:\n    steps: [{ step: one, do: x, what_if: [b, c] }, { step: two, do: y }]\n  b:\n    skip: later\n    steps: [{ step: three, what_if: [d] }]\n  c:\n    steps: [{ step: four }]\n  d:\n    steps: [{ step: five }]\n";
    let tree = plan_yaml(yaml).unwrap();
    assert_eq!(tree.step_count(), 3);
}

#[test]
fn plan_errors_name_the_file_and_scenario() {
    let shadow = "vars:\n  acme: { record: loan, id: a }\nscenarios:\n  s:\n    steps: [{ step: one, do: x, vars: { acme: { record: loan, from: result } } }]\n";
    let message = err(shadow);
    assert!(message.contains("shadows"), "{message}");
    assert!(message.contains(r#"file: "t.yaml""#), "{message}");
    assert!(message.contains(r#"scenario: "s""#), "{message}");
    let cycle = err("scenarios:\n  a:\n    extends: a\n    steps: [{ step: one }]\n");
    assert!(cycle.contains(r#"scenario: "a""#), "{cycle}");
    let empty = err("scenarios:\n  a:\n    steps: [{ step: one, do: x, fails: \"\" }]\n");
    assert!(empty.contains("`fails:` needs the text"), "{empty}");
}

#[test]
fn a_scenario_named_twice_runs_from_each() {
    let yaml = r#"
scenarios:
  a:
    steps:
      - { step: one, do: x, what_if: [b] }
      - { step: two, do: y, what_if: [b] }
  b:
    steps:
      - step: b's own
"#;
    let tree = plan_yaml(yaml).unwrap();
    assert_eq!(branch_names(&tree, 0, 0), ["b"]);
    assert_eq!(branch_names(&tree, 0, 1), ["b"]);
    let Branch::Scenario(from_one) = &tree.roots[0].steps[0].what_ifs[0] else {
        panic!()
    };
    let Branch::Scenario(from_two) = &tree.roots[0].steps[1].what_ifs[0] else {
        panic!()
    };
    assert!(matches!(&from_one.steps[0].step.do_, Some(Do::Script(s)) if s == "x"));
    assert!(matches!(&from_two.steps[0].step.do_, Some(Do::Script(s)) if s == "y"));
}

#[test]
fn extends_overrides_settings_and_appends_steps() {
    let yaml = r#"
scenarios:
  funded:
    session: { app: portal, investor_id: $carol, owner: carol }
    clock: 2026-11-14T09:00:00Z
    steps:
      - { step: Carol bids, do: x, what_if: [no-cash, too-late] }
  no-cash:
    steps:
      - { step: Carol has no funds, do: y }
  too-late:
    steps:
      - { step: Carol is late, do: z }
  funded-by-dave:
    extends: funded
    session: { app: portal, investor_id: $dave, owner: dave }
    steps:
      - { step: Dave closes, do: w }
"#;
    let tree = plan_yaml(yaml).unwrap();
    let dave = &tree.roots[1];
    let Some(Session::Api { fields, .. }) = &dave.scenario.session else {
        panic!()
    };
    assert_eq!(fields["owner"], "dave");
    assert_eq!(dave.scenario.clock.as_deref(), Some("2026-11-14T09:00:00Z"));
    assert_eq!(dave.steps.len(), 2);
    assert_eq!(branch_names(&tree, 1, 0), ["no-cash", "too-late"]);
    assert!(
        matches!(&dave.steps[0].step.session, Some(Session::Api { fields, .. }) if fields["owner"] == "dave")
    );

    let yaml = "scenarios:\n  a:\n    stub: { s: 1, t: 1 }\n    steps: [{ step: one }]\n  b:\n    extends: a\n    stub: { t: 2 }\n    steps: [{ step: two }]\n";
    let b = &plan_yaml(yaml).unwrap().roots[1];
    let sentences: Vec<_> = b.steps.iter().map(|s| s.step.step.as_str()).collect();
    assert_eq!(sentences, ["one", "two"]);
    assert_eq!(
        (b.scenario.stub["s"].as_i64(), b.scenario.stub["t"].as_i64()),
        (Some(1), Some(2))
    );
}

#[test]
fn duplicate_step_sentences_are_refused() {
    let own = "scenarios:\n  a:\n    steps: [{ step: one }, { step: one }]\n";
    assert!(err(own).contains("appears twice"));
    let inherited = "scenarios:\n  a:\n    steps: [{ step: one }]\n  b:\n    extends: a\n    steps: [{ step: one }]\n";
    assert!(err(inherited).contains("appears twice"));
}

#[test]
fn an_extends_cycle_is_refused() {
    let yaml = "scenarios:\n  a:\n    extends: b\n    steps: [{ step: one }]\n  b:\n    extends: a\n    steps: [{ step: two }]\n";
    assert!(err(yaml).contains("`extends` cycle"));
}

#[test]
fn a_clock_offset_parses() {
    let parsed: Vec<ClockOffset> = ["+2d", "+1mo", "+90m", "+3h", "+1w"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    let units: Vec<_> = parsed.iter().map(|o| (o.amount, o.unit)).collect();
    assert_eq!(
        units,
        [
            (2, ClockUnit::Days),
            (1, ClockUnit::Months),
            (90, ClockUnit::Minutes),
            (3, ClockUnit::Hours),
            (1, ClockUnit::Weeks)
        ]
    );
    assert_eq!(parsed[1].to_string(), "+1mo");
    for bad in ["2d", "+d", "+2", "+2y", "-2d"] {
        assert!(bad.parse::<ClockOffset>().is_err(), "accepted {bad}");
    }
    assert!(plan_yaml("scenarios:\n  a:\n    steps: [{ step: one, clock: 2d }]\n").is_err());
}
