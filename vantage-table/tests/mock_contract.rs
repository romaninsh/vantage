//! MockTableSource meets the write contract.

use serde_json::{Value, json};
use vantage_dataset::Inserted;
use vantage_dataset::contract::{self, Fixture};
use vantage_expressions::{Expression, ExpressiveEnum};
use vantage_table::mocks::mock_table_source::MockTableSource;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};

fn eq(col: &str, v: Value) -> Expression<Value> {
    Expression::new(
        "{} = {}",
        vec![
            ExpressiveEnum::Nested(Expression::new(col, vec![])),
            ExpressiveEnum::Nested(Expression::new("{}", vec![ExpressiveEnum::Scalar(v)])),
        ],
    )
}

async fn tables() -> (
    Table<MockTableSource, EmptyEntity>,
    Table<MockTableSource, EmptyEntity>,
) {
    let src = MockTableSource::new()
        .with_data(
            "item",
            vec![
                json!({"id": "in1", "name": "a", "parent": "p1"}),
                json!({"id": "out1", "name": "b", "parent": "p2"}),
            ],
        )
        .await;
    let all = Table::new("item", src.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("parent")
        .with_column_of::<i64>("price");
    let set = all.clone().with_condition(eq("parent", json!("p1")));
    (all, set)
}

macro_rules! mock_check {
    ($name:ident, $check:path) => {
        #[tokio::test]
        async fn $name() {
            let (all, set) = tables().await;
            $check(&Fixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text: |s| json!(s),
                detects_outside: true,
            })
            .await;
        }
    };
}

mock_check!(mock_contract_insert, contract::check_insert);
mock_check!(mock_contract_patch, contract::check_patch);
mock_check!(mock_contract_delete_all, contract::check_delete_all);

#[tokio::test]
async fn mock_insert_table_value_reports_created_then_existed() {
    let (all, _) = tables().await;
    let src = all.data_source();
    let id = "k".to_string();

    let created = src
        .insert_table_value(&all, &id, &Record::from(json!({"name": "first"})))
        .await
        .unwrap();
    let Inserted::Created(stored) = created else {
        panic!("first insert is Created, got {created:?}");
    };
    assert_eq!(stored["name"], json!("first"));

    let again = src
        .insert_table_value(&all, &id, &Record::from(json!({"name": "second"})))
        .await
        .unwrap();
    assert_eq!(again, Inserted::Existed(stored), "returns the stored row");
}

#[tokio::test]
async fn mock_refuses_conditions_it_cannot_check() {
    let (all, _) = tables().await;
    let set = all.with_condition(vantage_expressions::expr_any!("price > {}", 10));
    contract::check_refused(&set, |s| s.to_string(), |s| json!(s)).await;
}
