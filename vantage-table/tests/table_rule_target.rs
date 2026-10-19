//! `TableRuleTarget`: a rule script's verbs land on a real table.

use ciborium::Value as CborValue;
use serde_json::{Value, json};
use vantage_dataset::prelude::*;
use vantage_rhai::{Env, Host, Limits};
use vantage_table::prelude::*;
use vantage_table::rule::{RuleCodec, TableRuleTarget};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::rhai::{
    RuleHandle, RuleTarget, RuleVerb, eval_rule_script, register_rule_vocab,
};

type MockTable = Table<MockTableSource, EmptyEntity>;

fn to_cbor(value: Value) -> CborValue {
    CborValue::serialized(&value).unwrap()
}

fn from_cbor(value: CborValue) -> Value {
    value.deserialized().unwrap()
}

// `RuleCodec::id_cbor` is `fn(&T::Id)`, and the mock's id is a `String`.
#[allow(clippy::ptr_arg)]
fn id_cbor(id: &String) -> CborValue {
    CborValue::Text(id.clone())
}

fn codec() -> RuleCodec<MockTableSource> {
    RuleCodec {
        to_cbor,
        from_cbor,
        id_cbor,
    }
}

async fn client() -> MockTable {
    let src = MockTableSource::new()
        .with_data(
            "client",
            vec![json!({"id": "1", "name": "a", "bakery_id": 3})],
        )
        .await;
    MockTable::new("client", src)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("bakery_id")
}

/// Run `code` as a rule over `table`; the table as the script left it, and
/// the verbs it recorded.
fn rule(table: MockTable, code: &str) -> vantage_core::Result<(MockTable, Vec<RuleVerb>)> {
    let target = TableRuleTarget::new(table, codec());
    let handle = RuleHandle::new(Box::new(target.clone()));
    let host = Host::builder(Limits::background())
        .vocab_fn(register_rule_vocab)
        .build();
    eval_rule_script(&host, code, Env::new(), &handle)?;
    Ok((target.into_table(), handle.verbs()))
}

#[tokio::test]
async fn rule_where_narrows_the_table() {
    let (table, verbs) = rule(
        client().await,
        r#"table.where("bakery_id", 3).where("name", "in", ["a", "b"])"#,
    )
    .unwrap();
    let conditions: Vec<String> = table.conditions().map(|c| c.preview()).collect();
    assert_eq!(conditions, ["bakery_id = 3", "name IN (\"a\", \"b\")"]);
    assert_eq!(table.invariants().get("bakery_id"), Some(&json!(3)));
    assert_eq!(
        verbs[0],
        RuleVerb::Where {
            column: "bakery_id".into(),
            described: "bakery_id = 3".into()
        }
    );
}

#[tokio::test]
async fn rule_where_with_an_unsupported_operator_names_it() {
    let err = rule(client().await, r#"table.where("bakery_id", ">", 3)"#)
        .err()
        .unwrap();
    assert!(err.to_string().contains("operator `>`"), "{err}");
}

#[tokio::test]
async fn rule_hooks_refuse_writes() {
    let (table, _) = rule(
        client().await,
        r#"
        table.before_insert(|record| if record.name == "bad" { throw "no bad names" });
        table.before_delete(|id| throw "viewers change nothing");
        "#,
    )
    .unwrap();
    let err = table
        .insert_value("2".to_string(), &Record::from(json!({"name": "bad"})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "no bad names");
    assert!(table.get_value("2".to_string()).await.unwrap().is_none());
    table
        .insert_value("3".to_string(), &Record::from(json!({"name": "good"})))
        .await
        .unwrap();

    let err = table.delete("1".to_string()).await.unwrap_err();
    assert_eq!(err.message(), "viewers change nothing");
    assert!(table.get_value("1".to_string()).await.unwrap().is_some());
}

#[tokio::test]
async fn rule_remove_column_reaches_the_table() {
    let (table, _) = rule(client().await, r#"table.remove_column("bakery_id")"#).unwrap();
    assert!(!table.columns().contains_key("bakery_id"));

    let err = rule(client().await, r#"table.remove_column("id")"#)
        .err()
        .unwrap();
    assert!(err.to_string().contains("is the id column"), "{err}");
}

#[tokio::test]
async fn a_native_condition_of_another_type_is_an_error() {
    let mut target = TableRuleTarget::new(client().await, codec());
    let err = target
        .native_condition(Box::new(7_i64), "7".into())
        .unwrap_err();
    assert!(err.to_string().contains("native condition"), "{err}");
}
