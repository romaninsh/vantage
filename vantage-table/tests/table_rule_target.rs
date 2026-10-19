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

/// The review's case: an editor allowed only bakery 3 patches a bakery-4
/// row into bakery 3. The merged row says 3, so a check on `record` alone
/// passes; a closure that also checks `stored` refuses it.
#[tokio::test]
async fn an_update_closure_sees_the_stored_row() {
    let src = MockTableSource::new()
        .with_data(
            "client",
            vec![
                json!({"id": "1", "name": "a", "bakery_id": 3}),
                json!({"id": "2", "name": "b", "bakery_id": 4}),
            ],
        )
        .await;
    let table = MockTable::new("client", src)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("bakery_id");
    let (table, _) = rule(
        table,
        r#"
        let mine = [3];
        table.before_update(|record, stored| {
            if !mine.contains(record.bakery_id) { throw "not your bakery" }
            if stored != () && !mine.contains(stored.bakery_id) { throw "that row is not yours" }
        });
        table.before_save(|record, stored| if stored != () && stored.name != record.name && record.name == "x" { throw "save sees both" });
        "#,
    )
    .unwrap();

    let err = table
        .patch_value("2".to_string(), &Record::from(json!({"bakery_id": 3})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "that row is not yours");
    let row = table.get_value("2".to_string()).await.unwrap().unwrap();
    assert_eq!(row["bakery_id"], json!(4));

    let err = table
        .replace_value(
            "2".to_string(),
            &Record::from(json!({"name": "b", "bakery_id": 3})),
        )
        .await
        .unwrap_err();
    assert_eq!(err.message(), "that row is not yours");

    table
        .patch_value("1".to_string(), &Record::from(json!({"name": "z"})))
        .await
        .unwrap();
    let err = table
        .patch_value("1".to_string(), &Record::from(json!({"name": "x"})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "save sees both");

    // An insert has no stored row: the closure gets unit.
    table
        .insert_value(
            "3".to_string(),
            &Record::from(json!({"name": "x", "bakery_id": 3})),
        )
        .await
        .unwrap();
}

/// The spec's editor rule: a patch that doesn't name `bakery_id` is judged
/// on the stored row, so it passes; one that moves the row out is refused.
#[tokio::test]
async fn rule_update_hooks_judge_the_full_row() {
    let (table, _) = rule(
        client().await,
        r#"
        let mine = [3];
        table.before_update(|record| if !mine.contains(record.bakery_id) { throw "not your bakery" });
        table.before_save(|record| if record.name == () { throw "no name" });
        "#,
    )
    .unwrap();
    table
        .patch_value("1".to_string(), &Record::from(json!({"name": "z"})))
        .await
        .unwrap();
    let err = table
        .patch_value("1".to_string(), &Record::from(json!({"bakery_id": 4})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "not your bakery");
    let row = table.get_value("1".to_string()).await.unwrap().unwrap();
    assert_eq!(row["name"], json!("z"));
    assert_eq!(row["bakery_id"], json!(3));
}

/// A replace is the whole new row: the script sees exactly what is written,
/// with nothing left over from the stored row. A patch is merged.
#[tokio::test]
async fn a_replace_is_judged_on_the_written_record_only() {
    let (table, _) = rule(
        client().await,
        r#"
        table.before_update(|record| if record.bakery_id != () { throw "bakery_id is still there" });
        table.before_save(|record| if record.bakery_id != () { throw "save sees bakery_id" });
        "#,
    )
    .unwrap();
    table
        .replace_value("1".to_string(), &Record::from(json!({"name": "z"})))
        .await
        .unwrap();
    let row = table.get_value("1".to_string()).await.unwrap().unwrap();
    assert_eq!(Value::from(row), json!({"id": "1", "name": "z"}));

    let err = table
        .patch_value("1".to_string(), &Record::from(json!({"bakery_id": 3})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "bakery_id is still there");

    let (table, _) = rule(
        client().await,
        r#"table.before_save(|record| if record.bakery_id != () { throw "save sees bakery_id" });"#,
    )
    .unwrap();
    let err = table
        .patch_value("1".to_string(), &Record::from(json!({"name": "y"})))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "save sees bakery_id");
}

#[tokio::test]
async fn rule_delete_hook_sees_the_stored_row() {
    let (table, _) = rule(
        client().await,
        r#"table.before_delete(|id, row| if row.name == "a" { throw `keep ${id}` });"#,
    )
    .unwrap();
    let err = table.delete("1".to_string()).await.unwrap_err();
    assert_eq!(err.message(), "keep 1");
    assert!(table.get_value("1".to_string()).await.unwrap().is_some());
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
