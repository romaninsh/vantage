use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use vantage_action::{ActionOutput, ActionTarget, Target, bare_key, record_action, table_action};

#[derive(Deserialize, JsonSchema)]
struct Rename {
    name: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Serialize, JsonSchema)]
struct Renamed {
    label: String,
}

fn rename_record() -> std::sync::Arc<dyn vantage_action::ModelAction> {
    record_action(
        "rename",
        "tag",
        "Renames a tag.",
        |id, r: Rename| async move {
            Ok(Renamed {
                label: format!("{id}={}{}", r.name, r.note.unwrap_or_default()),
            })
        },
    )
}

fn rename_table() -> std::sync::Arc<dyn vantage_action::ModelAction> {
    table_action(
        "rename_all",
        "tag",
        "Renames all.",
        |r: Rename| async move { Ok(Renamed { label: r.name }) },
    )
}

#[test]
fn into_value_unwraps_a_value_output() {
    let out = ActionOutput::Value(serde_json::json!({"a": 1}));
    assert_eq!(out.into_value(), serde_json::json!({"a": 1}));
}

#[test]
fn into_value_of_a_record_output_is_its_value() {
    let out = ActionOutput::Record {
        table: "tag".into(),
        id: "GH1".into(),
        value: serde_json::json!({"name": "x"}),
    };
    assert_eq!(out.into_value(), serde_json::json!({"name": "x"}));
}

#[test]
fn action_target_reports_its_table_for_both_variants() {
    assert_eq!(
        ActionTarget::Table {
            table: "tag".into()
        }
        .table(),
        "tag"
    );
    assert_eq!(
        ActionTarget::Record {
            table: "tag".into()
        }
        .table(),
        "tag"
    );
}

#[test]
fn bare_key_strips_only_the_table_prefix() {
    assert_eq!(bare_key("tag", "tag:GH123"), "GH123");
    assert_eq!(bare_key("tag", "GH123"), "GH123");
    assert_eq!(bare_key("tag", "taggy"), "taggy");
}

#[tokio::test]
async fn record_action_round_trips_with_a_qualified_id() {
    let out = rename_record()
        .invoke(Target::Id("tag:7".into()), serde_json::json!({"name": "n"}))
        .await
        .unwrap();
    assert_eq!(out.into_value(), serde_json::json!({"label": "7=n"}));
}

#[tokio::test]
async fn record_action_without_an_id_is_incorrect_usage() {
    let err = rename_record()
        .invoke(Target::None, serde_json::json!({"name": "n"}))
        .await
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err}");
}

#[tokio::test]
async fn table_action_with_an_id_is_incorrect_usage() {
    let err = rename_table()
        .invoke(Target::Id("tag:7".into()), serde_json::json!({"name": "n"}))
        .await
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err}");
}

#[tokio::test]
async fn record_action_rejects_malformed_input_as_incorrect_usage() {
    let err = rename_record()
        .invoke(Target::Id("7".into()), serde_json::json!({"note": "x"}))
        .await
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err}");
    assert!(err.to_string().contains("invalid action input"), "{err}");
}

#[tokio::test]
async fn table_action_rejects_malformed_input_as_incorrect_usage() {
    let err = rename_table()
        .invoke(Target::None, serde_json::json!({"name": 5}))
        .await
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err}");
}

#[tokio::test]
async fn an_error_from_the_body_is_not_classified_as_incorrect_usage() {
    let action = record_action("boom", "tag", "Fails.", |_id, _r: Rename| async move {
        Err::<Renamed, _>(vantage_core::error!("backend down"))
    });
    let err = action
        .invoke(Target::Id("7".into()), serde_json::json!({"name": "n"}))
        .await
        .unwrap_err();
    assert!(!err.is_incorrect_usage(), "{err}");
}

#[test]
fn input_schema_marks_only_non_optional_fields_required() {
    let schema = serde_json::to_value(&rename_record().descriptor().input_schema).unwrap();
    let required = schema["required"].as_array().unwrap();
    assert!(required.iter().any(|v| v == "name"));
    assert!(!required.iter().any(|v| v == "note"));
}
