//! Live integration tests against the `vantage-demo-products` DynamoDB
//! table provisioned by `test-tf/dynamodb.tf`.
//!
//! Skipped (`return Ok(())`) when AWS credentials aren't configured, so
//! CI without AWS access stays green. Locally:
//!
//! ```sh
//! cd test-tf && tofu apply
//! cd ../vantage-aws && cargo test --test dynamodb_live -- --test-threads=1
//! ```
//!
//! Tests use a per-run id prefix so concurrent runs (or rerun-after-failure
//! with leftover items) don't collide. Each test cleans up after itself.

use std::time::{SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;
use vantage_aws::AwsAccount;
use vantage_aws::dynamodb::{AnyDynamoType, AttributeValue, DynamoDB, DynamoId};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

const PRODUCTS_TABLE: &str = "vantage-demo-products";
/// Region the `test-tf` stack provisions DynamoDB tables in. Pinned here
/// because the developer's `~/.aws/config` default may point elsewhere.
const TEST_REGION: &str = "eu-west-2";

fn account_or_skip() -> Option<AwsAccount> {
    AwsAccount::from_default()
        .ok()
        .map(|a| a.with_region(TEST_REGION))
}

/// Build a unique id prefix for one test run so cleanup of leftovers
/// from a panicking test doesn't fight a concurrent test.
fn run_prefix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("vt-test-{}", nanos)
}

fn products_table(db: DynamoDB) -> Table<DynamoDB, EmptyEntity> {
    Table::new(PRODUCTS_TABLE, db).with_id_column("id")
}

fn build_record(id: &str, name: &str, price: i64) -> Record<AnyDynamoType> {
    let mut rec = Record::new();
    rec.insert(
        "id".into(),
        AnyDynamoType::untyped(AttributeValue::S(id.to_string())),
    );
    rec.insert(
        "name".into(),
        AnyDynamoType::untyped(AttributeValue::S(name.to_string())),
    );
    rec.insert(
        "price".into(),
        AnyDynamoType::untyped(AttributeValue::N(price.to_string())),
    );
    rec
}

#[tokio::test]
async fn insert_get_delete_round_trip() -> vantage_core::Result<()> {
    let Some(aws) = account_or_skip() else {
        eprintln!("skipping: AWS credentials not configured");
        return Ok(());
    };
    let db = DynamoDB::new(aws);
    let table = products_table(db);

    let prefix = run_prefix();
    let id = DynamoId::new(format!("{}-cupcake", prefix));
    let rec = build_record(id.as_str(), "Cupcake", 120);

    let written = table.insert_value(&id, &rec).await?;
    assert_eq!(
        written["name"].try_get::<String>(),
        Some("Cupcake".to_string()),
        "insert returned the wrong name"
    );
    assert_eq!(
        written["price"].try_get::<i64>(),
        Some(120),
        "insert returned the wrong price"
    );

    let fetched = table
        .get_value(&id)
        .await?
        .expect("just-inserted item should be found");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Cupcake".into()));

    table.delete(&id).await?;
    let after_delete = table.get_value(&id).await?;
    assert!(after_delete.is_none(), "item should be gone after delete");

    Ok(())
}

#[tokio::test]
async fn list_returns_inserted_items() -> vantage_core::Result<()> {
    let Some(aws) = account_or_skip() else {
        eprintln!("skipping: AWS credentials not configured");
        return Ok(());
    };
    let db = DynamoDB::new(aws);
    let table = products_table(db);

    let prefix = run_prefix();
    let ids: Vec<DynamoId> = (0..3)
        .map(|i| DynamoId::new(format!("{}-{}", prefix, i)))
        .collect();

    for (i, id) in ids.iter().enumerate() {
        let rec = build_record(id.as_str(), &format!("Item-{}", i), (i as i64) * 10);
        table.insert_value(id, &rec).await?;
    }

    let all: IndexMap<DynamoId, Record<AnyDynamoType>> = table.list_values().await?;
    let all_ids: std::collections::HashSet<&DynamoId> = all.keys().collect();
    let inserted_count = ids.iter().filter(|id| all_ids.contains(id)).count();
    assert_eq!(
        inserted_count,
        3,
        "expected 3 freshly-inserted items in scan; saw {} of them (table has {} total)",
        inserted_count,
        all.len()
    );

    // Cleanup.
    for id in &ids {
        table.delete(id).await?;
    }
    Ok(())
}

#[tokio::test]
async fn replace_overwrites_existing_item() -> vantage_core::Result<()> {
    let Some(aws) = account_or_skip() else {
        eprintln!("skipping: AWS credentials not configured");
        return Ok(());
    };
    let db = DynamoDB::new(aws);
    let table = products_table(db);

    let id = DynamoId::new(format!("{}-replace", run_prefix()));

    table
        .insert_value(&id, &build_record(id.as_str(), "Original", 100))
        .await?;

    table
        .replace_value(&id, &build_record(id.as_str(), "Updated", 200))
        .await?;

    let fetched = table
        .get_value(&id)
        .await?
        .expect("item should exist after replace");
    assert_eq!(fetched["name"].try_get::<String>(), Some("Updated".into()));
    assert_eq!(fetched["price"].try_get::<i64>(), Some(200));

    table.delete(&id).await?;
    Ok(())
}

#[tokio::test]
async fn get_value_returns_none_for_missing_id() -> vantage_core::Result<()> {
    let Some(aws) = account_or_skip() else {
        eprintln!("skipping: AWS credentials not configured");
        return Ok(());
    };
    let db = DynamoDB::new(aws);
    let table = products_table(db);

    let missing = DynamoId::new(format!("{}-does-not-exist", run_prefix()));
    let result = table.get_value(&missing).await?;
    assert!(result.is_none(), "missing id should return None, not error");
    Ok(())
}
