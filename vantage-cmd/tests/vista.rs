//! The cmd `TableShell` over local shell-script fixtures: narrowing,
//! windows and traversal.

use ciborium::Value as CborValue;
use vantage_cmd::{Cmd, CmdSpec};
use vantage_core::ErrorKind;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Capability, FilterOp, Vista};

fn fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Rows `a` and `b` from the list script; `detail` is the DETAIL script.
fn two_pass(detail: &str) -> Vista {
    use vantage_vista::VistaFactory;

    let yaml = format!(
        r#"
name: items
columns:
  id: {{ type: string, flags: [id] }}
  detail: {{ type: string }}
cmd:
  rhai: |
    parse_json(run(["list"]).stdout)
  detail: |
    {detail}
"#
    );
    Cmd::new(fixture("role.sh"))
        .vista_factory()
        .from_yaml(&yaml)
        .unwrap()
}

#[tokio::test]
async fn detail_get_stays_inside_a_narrowed_set() {
    let vista = two_pass(r#"parse_json(run(["detail", id]).stdout)"#)
        .with_condition("id", FilterOp::Eq, "a")
        .unwrap();
    assert!(vista.get_value("b").await.unwrap().is_none());
    let a = vista.get_value("a").await.unwrap().unwrap();
    assert_eq!(a.get("detail"), Some(&CborValue::from("full-a")));
}

/// A DETAIL script that returns only the extra fields, with no id, answers
/// for the id it was asked about.
#[tokio::test]
async fn detail_get_stamps_a_single_id_less_row() {
    let vista = two_pass(r#"[#{ "detail": "x" }]"#);
    let a = vista.get_value("a").await.unwrap().unwrap();
    assert_eq!(a.get("detail"), Some(&CborValue::from("x")));
    assert_eq!(a.get("id"), Some(&CborValue::from("a")));

    let several = two_pass(r#"[#{ "detail": "x" }, #{ "detail": "y" }]"#);
    assert!(several.get_value("a").await.unwrap().is_none());
}

#[tokio::test]
async fn detail_get_drops_a_row_for_another_id() {
    // This DETAIL script always answers with row `a`, whatever it is asked.
    let vista = two_pass(r#"parse_json(run(["detail", "a"]).stdout)"#);
    assert!(vista.get_value("missing").await.unwrap().is_none());
    assert!(vista.get_value("a").await.unwrap().is_some());
}

/// Rows `alpha` and `beta`, keyed by `name`, with an `args` has-many whose
/// script passes each condition value to the command, which echoes it back
/// as a row keyed by `arg`. The join column `owner` is a request flag, so
/// it is declared `RequestParam`: the echoed rows don't carry it.
fn items() -> Vista {
    let list = r#"parse_json(run([]).stdout).items"#;
    let echo_args = r#"
        let args = [];
        for c in conditions { args += [c.value]; }
        parse_json(run(args).stdout)
    "#;
    let args_command = fixture("args.sh");
    let cmd = Cmd::new(fixture("echo_json.sh")).with_script("items", list);
    let table = Table::<Cmd, EmptyEntity>::new("items", cmd.clone())
        .with_id_column("name")
        .with_column_of::<i64>("size")
        .with_many("args", "owner", move |cmd: Cmd| {
            let spec = CmdSpec::new(echo_args).with_command(args_command.clone());
            let cmd = cmd.with_table("args", spec);
            Table::<Cmd, EmptyEntity>::new("args", cmd)
                .with_id_column("arg")
                .with_column(Column::<String>::new("owner").with_flag(ColumnFlag::RequestParam))
        });
    cmd.vista_factory().from_table(table).unwrap()
}

#[tokio::test]
async fn cmd_advertises_windows_and_traversal_only() {
    let caps: Vec<Capability> = items().capabilities().iter().collect();
    assert_eq!(caps, [Capability::Window, Capability::Traverse]);
}

#[tokio::test]
async fn cmd_window_slices_listed_rows() {
    let page = items().fetch_window(1, 10).await.unwrap();
    let ids: Vec<&str> = page.rows.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["beta"]);
    assert_eq!(page.total, Some(2));
}

#[tokio::test]
async fn cmd_refuses_conditions_it_cant_apply() {
    let mut vista = items();
    let op = vista
        .add_condition("size", FilterOp::Gt, 1_i64)
        .unwrap_err();
    assert_eq!(op.kind(), ErrorKind::Unsupported);
    let unknown = vista
        .add_condition("colour", FilterOp::Eq, "red")
        .unwrap_err();
    assert_ne!(unknown.kind(), ErrorKind::Unsupported);
    assert!(vista.query().is_empty());
}

/// Row `a` has `kind` and `note`, row `b` lacks both, and neither carries
/// the `mode` request flag. `note` is hidden: not shown, but a row field.
fn rides() -> Vista {
    use vantage_vista::VistaFactory;

    let yaml = r#"
name: rides
columns:
  id: { type: string, flags: [id] }
  kind: { type: string }
  note: { type: string, flags: [hidden] }
  mode: { type: string, flags: [request_param] }
cmd:
  rhai: |
    [#{ "id": "a", "kind": "ride", "note": "x" }, #{ "id": "b" }]
"#;
    Cmd::new(fixture("echo_json.sh"))
        .vista_factory()
        .from_yaml(yaml)
        .unwrap()
}

#[tokio::test]
async fn cmd_row_missing_a_visible_filtered_column_is_excluded() {
    let vista = rides()
        .with_condition("kind", FilterOp::Eq, "ride")
        .unwrap();
    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"]);
}

#[tokio::test]
async fn cmd_row_missing_a_hidden_filtered_column_is_excluded() {
    let vista = rides().with_condition("note", FilterOp::Eq, "x").unwrap();
    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"]);
}

#[tokio::test]
async fn cmd_row_missing_a_request_param_filtered_column_is_kept() {
    let vista = rides()
        .with_condition("mode", FilterOp::Eq, "summary")
        .unwrap();
    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a", "b"]);
}

/// The bundled relation targets declare their join columns as request flags
/// that are also hidden: rows don't carry them, and grids don't show them.
#[test]
fn bundled_request_flag_columns_are_hidden_request_params() {
    use vantage_cmd::models::CmdModelFactory;
    use vantage_vista::ColumnFlag as VistaFlag;

    let factory = CmdModelFactory::new(Cmd::new(fixture("echo_json.sh")));
    let columns = [
        ("ecs.services", "cluster"),
        ("ecs.tasks", "cluster"),
        ("iam.access_keys", "UserName"),
        ("iam.user_groups", "UserName"),
        ("iam.user_policies", "UserName"),
        ("lambda.aliases", "FunctionName"),
        ("lambda.versions", "FunctionName"),
        ("log.events", "logGroupName"),
        ("log.streams", "logGroupName"),
        ("s3.objects", "Bucket"),
    ];
    for (model, column) in columns {
        let (vista, _) = factory.for_name(model).expect(model);
        let column = vista.column(column).expect(column);
        assert!(column.has_flag(VistaFlag::Hidden), "{model}");
        assert!(column.has_flag(VistaFlag::RequestParam), "{model}");
    }
}

/// A script that ignores its conditions still can't widen the set: `In` is
/// re-applied to the rows it returns, with the same missing-field rule as
/// `Eq`.
#[tokio::test]
async fn cmd_reapplies_an_in_condition_the_script_ignores() {
    let script =
        r#"[#{ "id": "a", "kind": "ride" }, #{ "id": "b", "kind": "walk" }, #{ "id": "c" }]"#;
    let cmd = Cmd::new(fixture("echo_json.sh")).with_script("rides", script);
    let table = Table::<Cmd, EmptyEntity>::new("rides", cmd)
        .with_id_column("id")
        .with_column_of::<String>("kind")
        .with_condition(vantage_cmd::CmdCondition::in_("kind", ["ride", "bike"]));
    let rows = table.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"]);
}

#[tokio::test]
async fn cmd_get_lists_and_picks_without_a_detail_script() {
    let vista = items()
        .with_condition("name", FilterOp::Eq, "beta")
        .unwrap();
    assert!(vista.get_value("beta").await.unwrap().is_some());
    assert!(vista.get_value("alpha").await.unwrap().is_none());
}

/// Every bundled relation names its target by the target's table name.
/// Joins are strict, so the target must declare the column its join key
/// constrains. Building a target runs no command.
#[test]
fn bundled_relation_targets_are_named_and_declare_their_join_columns() {
    use vantage_cmd::models::CmdModelFactory;

    let factory = CmdModelFactory::new(Cmd::new("aws"));
    for name in CmdModelFactory::known_names() {
        let (vista, _) = factory
            .for_name(name)
            .unwrap_or_else(|| panic!("{name} must build"));
        for reference in vista.references().values() {
            let target = vista.get_ref(&reference.name, None).unwrap();
            assert_eq!(reference.target, target.name(), "{name}:{}", reference.name);
            for key in &reference.keys {
                let column = key.target_column(&target).unwrap();
                assert!(
                    target.column(column).is_some(),
                    "{name}:{} target `{}` lacks join column `{column}`",
                    reference.name,
                    target.name()
                );
            }
        }
    }
}

#[tokio::test]
async fn cmd_has_many_narrows_the_target_by_the_parent_row() {
    let vista = items();
    let row: Record<CborValue> = [("name".to_string(), CborValue::from("beta"))]
        .into_iter()
        .collect();
    let target = vista.get_ref("args", Some(&row)).unwrap();
    assert!(target.is_narrowed());
    assert_eq!(target.query().conditions[0].column, "owner");

    let rows = target.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["beta"]);
}
