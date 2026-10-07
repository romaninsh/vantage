use std::fs;
use std::path::Path;
use std::time::Duration;

use super::*;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

const BASIC: &str = r#"
seed: 7
tables:
  ticket:
    count: 20
    columns:
      status: { faker: { pick: { values: [Open, Closed] } } }
      amount: { type: int, faker: { range: { min: 1, max: 500 } } }
      note: {}
  audit: { count: 0, columns: { what: {} } }
sims:
  churn:
    table: ticket
    script: !include churn.rhai
    clock: 10
    warm: 2h
    spawn: { burst: 3, rate: 1.5, max: 10, args: { who: bob } }
  audit: { script: "sleep(seconds(1));" }
stress:
  duration: 5s
  limits: { cpu_pct: 400 }
"#;

#[test]
fn loads_tables_sims_and_includes() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(
        root.path(),
        "basic/churn.rhai",
        "let id = table().insert(#{});",
    );
    let s = load(root.path(), "basic").unwrap();
    assert_eq!(s.seed, Some(7));
    assert_eq!(s.tables["ticket"].count, Some(20));
    assert_eq!(s.sims["churn"].script, "let id = table().insert(#{});");
    assert_eq!(s.duration().unwrap(), Duration::from_secs(5));
    assert_eq!(s.stress.limits.cpu_pct, Some(400.0));
}

#[test]
fn sim_defs_apply_vantage_ui_defaults() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let defs = load(root.path(), "basic").unwrap().sim_defs().unwrap();
    let churn = &defs[0];
    assert_eq!(
        (churn.spawn.burst, churn.spawn.rate_per_min, churn.spawn.max),
        (3, 1.5, 10)
    );
    assert_eq!(churn.clock, 10.0);
    assert_eq!(churn.warm, Some(Duration::from_secs(7200)));
    assert_eq!(churn.spawn.args["who"], "bob");
    let audit = &defs[1];
    assert_eq!(
        (audit.spawn.burst, audit.spawn.rate_per_min, audit.spawn.max),
        (1, 0.0, 1)
    );
    assert_eq!(audit.clock, 1.0);
}

#[test]
fn sim_without_table_uses_first_table() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let defs = load(root.path(), "basic").unwrap().sim_defs().unwrap();
    assert_eq!(defs[1].table, "ticket");
}

#[test]
fn include_may_reach_a_sibling_scenario() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "shared/steady.rhai", "sleep(seconds(1));");
    write(
        root.path(),
        "chaos/x/scenario.yaml",
        "tables: { row: { count: 0 } }\nsims: { s: { script: !include ../../shared/steady.rhai } }\n",
    );
    let s = load(root.path(), "chaos/x").unwrap();
    assert_eq!(s.sims["s"].script, "sleep(seconds(1));");
}

#[test]
fn include_outside_root_is_refused() {
    let outer = tempfile::tempdir().unwrap();
    write(outer.path(), "secret.rhai", "x");
    let root = outer.path().join("scenarios");
    write(
        &root,
        "a/scenario.yaml",
        "sims: { s: { script: !include ../../secret.rhai } }\n",
    );
    let err = load(&root, "a").unwrap_err();
    assert!(err.contains("outside"), "{err}");
}

#[test]
fn missing_include_names_the_file() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "a/scenario.yaml",
        "sims: { s: { script: !include nope.rhai } }\n",
    );
    let err = load(root.path(), "a").unwrap_err();
    assert!(err.contains("nope.rhai"), "{err}");
}

#[test]
fn unknown_key_names_the_scenario() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a/scenario.yaml", "sims: {}\nbogus: 1\n");
    let err = load(root.path(), "a").unwrap_err();
    assert!(
        err.contains("a/scenario.yaml") && err.contains("bogus"),
        "{err}"
    );
}

#[test]
fn durations_parse_like_vantage_ui() {
    assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
    assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
    assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
    assert_eq!(parse_duration("12h").unwrap(), Duration::from_secs(43_200));
    assert_eq!(parse_duration("3d").unwrap(), Duration::from_secs(259_200));
    assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
    assert!(parse_duration("soon").is_err());
}

#[test]
fn scale_multiplies_sims_and_rows() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let s = load(root.path(), "basic").unwrap().scaled(2.0, 3.0);
    let churn = &s.sims["churn"].spawn;
    assert_eq!((churn.burst, churn.max), (Some(6), Some(20)));
    assert_eq!(churn.rate, Some(3.0));
    assert_eq!(
        s.sims["audit"].spawn.rate, None,
        "an absent rate stays absent"
    );
    assert_eq!(s.tables["ticket"].count, Some(60));
}

#[test]
fn scale_multiplies_rate_without_rounding() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let mut s = load(root.path(), "basic").unwrap();
    assert_eq!(s.scaled(0.01, 1.0).sims["churn"].spawn.rate, Some(0.015));
    s.sims.get_mut("churn").unwrap().spawn.rate = Some(0.0);
    assert_eq!(s.scaled(5.0, 1.0).sims["churn"].spawn.rate, Some(0.0));
}

#[test]
fn scale_keeps_nonzero_at_least_one() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let s = load(root.path(), "basic").unwrap().scaled(0.01, 0.01);
    assert_eq!(s.sims["churn"].spawn.burst, Some(1));
    assert_eq!(s.sims["churn"].spawn.max, Some(1));
    assert_eq!(s.tables["ticket"].count, Some(1));
    assert_eq!(s.tables["audit"].count, Some(0));
}

#[test]
fn without_warm_clears_every_warm() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let s = load(root.path(), "basic").unwrap().without_warm();
    assert!(s.sims.values().all(|d| d.warm.is_none()));
}

#[test]
fn vista_metadata_puts_id_first() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let meta = load(root.path(), "basic").unwrap().vista_metadata("ticket");
    let names: Vec<_> = meta.columns.keys().map(String::as_str).collect();
    assert_eq!(names, ["id", "status", "amount", "note"]);
    assert_eq!(meta.id_column.as_deref(), Some("id"));
    assert_eq!(meta.columns["amount"].original_type, "int");
    assert!(meta.columns["id"].has_flag(vantage_vista::flags::ID));
    assert!(!meta.columns["note"].has_flag(vantage_vista::flags::ID));
}
