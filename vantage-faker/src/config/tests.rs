use vantage_memory::MemoryStore;

use super::*;

const YAML: &str = r#"
seed: 9
tables:
  client:
    count: 3
    columns:
      name: { type: string, faker: { pick: { values: [Ann, Bob] } } }
  invoice:
    count: 0
    indexed: [client_id]
    references: { client_id: client }
    fan_out: { column: client_id, min: 1, max: 2 }
    columns:
      client_id: {}
sims:
  pay:
    table: invoice
    script: "sleep(seconds(1));"
    clock: 10
    warm: 2h
    ops: 1000000
    spawn: { burst: 2, rate: 1.5, max: 4, args: { who: bob } }
  idle: { script: "sleep(seconds(1));" }
"#;

fn spec() -> DatasetSpec {
    serde_yaml_ng::from_str(YAML).unwrap()
}

#[test]
fn parses_and_generates() {
    let store = MemoryStore::new();
    spec().generate(&store).unwrap();
    assert_eq!(store.table("client").len(), 3);
    assert!((3..=6).contains(&store.table("invoice").len()));
    assert!(store.table("invoice").is_indexed("client_id"));
}

#[test]
fn unknown_keys_are_rejected() {
    let err = serde_yaml_ng::from_str::<DatasetSpec>("tables: { t: { cnt: 1 } }")
        .unwrap_err()
        .to_string();
    assert!(err.contains("cnt"), "{err}");
}

#[test]
fn unknown_reference_is_an_error() {
    let s: DatasetSpec = serde_yaml_ng::from_str(
        "tables: { a: { count: 1, references: { x_id: x }, columns: { x_id: {} } } }",
    )
    .unwrap();
    let err = s.generate(&MemoryStore::new()).unwrap_err().to_string();
    assert!(err.contains("a") && err.contains("x"), "{err}");
}

#[test]
fn durations_parse() {
    use std::time::Duration;
    assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
    assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
    assert!(parse_duration("soon").is_err());
}

#[cfg(feature = "sim")]
#[test]
fn sim_defs_apply_defaults() {
    let defs = spec().sim_defs().unwrap();
    let pay = &defs[0];
    assert_eq!(
        (pay.spawn.burst, pay.spawn.rate_per_min, pay.spawn.max),
        (2, 1.5, 4)
    );
    assert_eq!((pay.clock, pay.ops), (10.0, Some(1_000_000)));
    assert_eq!(pay.warm, Some(std::time::Duration::from_secs(7200)));
    assert_eq!(pay.spawn.args["who"], "bob");
    let idle = &defs[1];
    assert_eq!(idle.table, "client");
    assert_eq!(
        (
            idle.spawn.burst,
            idle.spawn.rate_per_min,
            idle.spawn.max,
            idle.clock
        ),
        (1, 0.0, 1, 1.0)
    );
}

#[cfg(feature = "sim")]
#[test]
fn bad_warm_duration_names_the_sim_and_key() {
    let s: DatasetSpec = serde_yaml_ng::from_str(
        "tables: { t: {} }\nsims: { courier: { script: \"sleep(seconds(1));\", warm: soon } }",
    )
    .unwrap();
    let err = s.sim_defs().unwrap_err().to_string();
    assert!(
        err.contains("courier") && err.contains("warm") && err.contains("soon"),
        "{err}"
    );
}

#[cfg(feature = "sim")]
#[test]
fn start_sims_runs_after_generate() {
    let store = MemoryStore::new();
    let s: DatasetSpec = serde_yaml_ng::from_str(
        "tables: { log: { count: 0 } }\nsims: { w: { script: \"insert(#{ who: 1 }); sleep(minutes(5));\" } }",
    )
    .unwrap();
    s.generate(&store).unwrap();
    let engine = s.start_sims(&store).unwrap().expect("an engine");
    engine.settle();
    assert_eq!(store.table("log").len(), 1);
    drop(engine);
    let empty: DatasetSpec = serde_yaml_ng::from_str("tables: { t: {} }").unwrap();
    assert!(empty.start_sims(&store).unwrap().is_none());
}

#[test]
fn table_spec_carries_weirdness_and_extra_fields() {
    let s: DatasetSpec = serde_yaml_ng::from_str(
        "seed: 1\ntables: { t: { count: 2, weirdness: 1.0, extra_fields: { count: 3, size: 16 } } }",
    )
    .unwrap();
    assert_eq!(s.tables["t"].weirdness, Some(1.0));
    let store = MemoryStore::new();
    s.generate(&store).unwrap();
    let row = store.table("t").get(&crate::seed_id(1)).unwrap();
    assert!(row.contains_key("extra_0003"), "{row:?}");
}

#[cfg(feature = "sim")]
#[test]
fn builtin_script_resolves_in_sim_defs() {
    let s: DatasetSpec =
        serde_yaml_ng::from_str("tables: { t: {} }\nsims: { a: { script: \"builtin:fifo\" } }")
            .unwrap();
    let defs = s.sim_defs().unwrap();
    assert_eq!(
        defs[0].script,
        crate::sim::builtin::builtin("fifo").unwrap()
    );
    let bad: DatasetSpec =
        serde_yaml_ng::from_str("tables: { t: {} }\nsims: { a: { script: \"builtin:nope\" } }")
            .unwrap();
    let err = bad.sim_defs().unwrap_err().to_string();
    assert!(err.contains("nope") && err.contains("fifo"), "{err}");
}

#[test]
fn new_matches_deserialized() {
    let parsed = spec();
    let built = DatasetSpec::new(parsed.seed, parsed.tables.clone(), parsed.sims.clone());
    assert_eq!(format!("{built:?}"), format!("{parsed:?}"));
}

#[cfg(feature = "sim")]
#[test]
fn sim_builder_takes_more_settings_before_start() {
    let store = MemoryStore::new();
    let s: DatasetSpec = serde_yaml_ng::from_str(
        "tables: { log: {} }\nsims: { w: { script: \"sleep(seconds(10)); insert(#{ who: 1 });\" } }",
    )
    .unwrap();
    s.generate(&store).unwrap();
    let start = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_790_553_600);
    let engine = s
        .sim_builder(&store)
        .unwrap()
        .expect("a builder")
        .manual_clock(start)
        .start()
        .unwrap();
    engine.settle();
    assert_eq!(
        store.table("log").len(),
        0,
        "the manual clock has not moved"
    );
    engine.advance(std::time::Duration::from_secs(10));
    engine.settle();
    assert_eq!(store.table("log").len(), 1);
}
