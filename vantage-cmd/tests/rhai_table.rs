//! End-to-end tests that don't touch the network: the "command" is a local
//! shell-script fixture, so the full path (conditions → Rhai → argv → run →
//! parse → records) is exercised deterministically.

use ciborium::Value as CborValue;
use vantage_cmd::{Cmd, eq};
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::VistaFactory;

fn fixtures_dir() -> String {
    format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn base_dir_resolves_relative_command_and_sets_cwd() {
    // A relative command path resolves against `base_dir`, and the child runs
    // with `base_dir` as its cwd — so the fixture can read its sibling file by
    // a relative path and echo it back as a row.
    let script = r#"
        let out = run([]);
        if out.exit_code != 0 { throw out.stderr; }
        parse_json(out.stdout).items
    "#;
    let cmd = Cmd::new("./read_sibling.sh")
        .with_base_dir(fixtures_dir())
        .with_script("items", script);
    let table = Table::<Cmd, EmptyEntity>::new("items", cmd).with_id_column("name");

    let rows = table.list_values().await.unwrap();
    assert!(
        rows.contains_key("cwd-sibling"),
        "got: {:?}",
        rows.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn lists_rows_from_command_output() {
    let script = r#"
        let out = run([]);
        if out.exit_code != 0 { throw out.stderr; }
        parse_json(out.stdout).items
    "#;
    let cmd = Cmd::new(format!("{}/echo_json.sh", fixtures_dir())).with_script("items", script);
    let table = Table::<Cmd, EmptyEntity>::new("items", cmd)
        .with_id_column("name")
        .with_column_of::<i64>("size");

    let rows = table.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains_key("alpha"));
    assert_eq!(rows["beta"]["size"], CborValue::from(2_i64));
}

#[tokio::test]
async fn conditions_reach_the_script_as_args() {
    let script = r#"
        let args = [];
        for c in conditions { args += [c.value]; }
        let out = run(args);
        parse_json(out.stdout)
    "#;
    let cmd = Cmd::new(format!("{}/args.sh", fixtures_dir())).with_script("a", script);
    let mut table = Table::<Cmd, EmptyEntity>::new("a", cmd).with_id_column("arg");
    table.add_condition(eq("anything", "hello"));

    let rows = table.list_values().await.unwrap();
    // The condition value was forwarded to the command as an argv element,
    // echoed back, and parsed into a row keyed by "arg".
    assert!(
        rows.contains_key("hello"),
        "got: {:?}",
        rows.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn client_side_eq_filter_narrows_rows() {
    // The script ignores conditions, so the client-side Eq safety net must
    // narrow to the matching row on a real record field.
    let script = r#"
        let out = run([]);
        parse_json(out.stdout).items
    "#;
    let cmd = Cmd::new(format!("{}/echo_json.sh", fixtures_dir())).with_script("items", script);
    let mut table = Table::<Cmd, EmptyEntity>::new("items", cmd)
        .with_id_column("name")
        .with_column_of::<i64>("size");
    table.add_condition(eq("name", "beta"));

    let rows = table.list_values().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows.contains_key("beta"));
}

#[tokio::test]
async fn writes_are_rejected() {
    let cmd = Cmd::new(format!("{}/echo_json.sh", fixtures_dir())).with_script("items", "[]");
    let table = Table::<Cmd, EmptyEntity>::new("items", cmd);
    let record: Record<CborValue> = Record::new();
    assert!(
        WritableValueSet::insert_value(&table, &"x".to_string(), &record)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn builds_vista_from_yaml() {
    let yaml = r#"
name: things
columns:
  id:
    type: string
    flags: [id, title]
  size:
    type: int
cmd:
  rhai: |
    parse_json(run([]).stdout)
"#;
    let cmd = Cmd::new("true");
    let vista = cmd.vista_factory().from_yaml(yaml).unwrap();
    assert_eq!(vista.get_id_column(), Some("id"));
    assert!(vista.get_column_names().contains(&"size"));
}
