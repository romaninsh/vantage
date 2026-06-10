//! Smoke test for the DynamoDB persistence wired into bakery_model3.
//!
//! Round-trips a typed `Product` (entity macro path) through the
//! `vantage-demo-products` table provisioned by `test-tf/dynamodb.tf`.
//!
//! ```sh
//! cd test-tf && tofu apply
//! cd ../bakery_model3 && cargo run --example dynamo-smoke
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use bakery_model3::*;
use vantage_aws::dynamodb::DynamoId;
use vantage_core::{Result, error};
use vantage_dataset::prelude::{ReadableDataSet, WritableDataSet};
use vantage_dataset::traits::WritableValueSet;

const TEST_REGION: &str = "eu-west-2";

#[tokio::main]
async fn main() -> Result<()> {
    let aws = AwsAccount::from_default()
        .map_err(|e| error!("AWS credentials not configured", details = e.to_string()))?
        .with_region(TEST_REGION);
    let db = DynamoDB::new(aws);

    let table = Product::dynamo_table(db);
    let id = DynamoId::new(format!(
        "smoke-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    let product = Product {
        name: "Smoke Cupcake".to_string(),
        calories: 250,
        price: 350,
        is_deleted: false,
        sticker: Some(Animal::Cat),
    };

    println!("→ insert {} ({})", product.name, id);
    table.insert(id.clone(), &product).await?;

    println!("→ get back {}", id);
    let fetched = table
        .get(id.clone())
        .await?
        .ok_or_else(|| error!("Product disappeared after insert"))?;

    assert_eq!(fetched.name, product.name);
    assert_eq!(fetched.calories, product.calories);
    assert_eq!(fetched.price, product.price);
    assert_eq!(fetched.is_deleted, product.is_deleted);
    assert_eq!(fetched.sticker, product.sticker);
    println!(
        "  ✓ all fields round-tripped (sticker = {:?})",
        fetched.sticker
    );

    println!("→ delete {}", id);
    table.delete(id.clone()).await?;

    let gone = table.get(id.clone()).await?;
    assert!(gone.is_none(), "Product should be gone after delete");
    println!("  ✓ confirmed gone");

    Ok(())
}
