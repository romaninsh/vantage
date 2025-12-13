//! Field querying example for vantage-surrealdb
//!
//! Run with:
//!   cargo run --example expr name     # returns string
//!   cargo run --example expr is_vip   # returns boolean
//!   cargo run --example expr age      # returns number
//!   cargo run --example expr foo      # error - no such field
//!
//! Requires SurrealDB running on cbor://localhost:8000

use std::env;
use surreal_client::SurrealConnection;
use vantage_core::Context;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::{surreal_expr, surrealdb::SurrealDB, types::AnySurrealType};

async fn setup_db() -> vantage_core::Result<SurrealDB> {
    let client = SurrealConnection::new()
        .url("cbor://localhost:8000/rpc")
        .namespace("example")
        .database("test")
        .auth_root("root", "root")
        .connect()
        .await
        .context("Failed to connect to SurrealDB")?;

    Ok(SurrealDB::new(client))
}

async fn setup_table(db: &SurrealDB) -> vantage_core::Result<()> {
    // Clean up any existing data
    let _ = db.execute(&surreal_expr!("DELETE demo_user")).await;

    // Create test record with name, is_vip, and age fields
    db.execute(&surreal_expr!(
        "CREATE demo_user:1 SET name = {}, is_vip = {}, age = {}",
        "Alice",
        true,
        25
    ))
    .await?;

    Ok(())
}

async fn execute_field_query(
    db: &SurrealDB,
    field_name: &str,
) -> vantage_core::Result<AnySurrealType> {
    let query = format!("SELECT VALUE {} FROM ONLY demo_user:1", field_name);
    let expr = vantage_expressions::Expression::<AnySurrealType>::new(query, vec![]);

    let result = db.execute(&expr).await?;
    Ok(result)
}

async fn query_field(db: &SurrealDB, field_name: &str) -> vantage_core::Result<()> {
    use vantage_core::{Context, error};

    let result = execute_field_query(db, field_name)
        .await
        .with_context(|| error!("Failed to access field", field = field_name))?;

    handle_result_value(field_name, result);
    Ok(())
}

fn handle_result_value(field_name: &str, value: AnySurrealType) {
    use vantage_surrealdb::types::SurrealTypeVariants;

    println!("Field '{}' found:", field_name);

    match value.type_variant() {
        Some(SurrealTypeVariants::String) => {
            if let Some(string_val) = value.try_get::<String>() {
                println!("  Type: String");
                println!("  Value: \"{}\"", string_val);
            }
        }
        Some(SurrealTypeVariants::Bool) => {
            if let Some(bool_val) = value.try_get::<bool>() {
                println!("  Type: Boolean");
                println!("  Value: {}", bool_val);
            }
        }
        Some(SurrealTypeVariants::Int) => {
            if let Some(int_val) = value.try_get::<i64>() {
                println!("  Type: Integer");
                println!("  Value: {}", int_val);
            }
        }
        Some(SurrealTypeVariants::Float) => {
            if let Some(float_val) = value.try_get::<f64>() {
                println!("  Type: Float");
                println!("  Value: {}", float_val);
            }
        }
        Some(other) => {
            println!("  Type: {:?}", other);
            println!("  Value: {:?}", value.value());
        }
        None => {
            println!("  Type: Unknown");
            println!("  Value: {:?}", value.value());
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let result = async {
        if args.len() != 2 {
            return Err(vantage_core::error!(
                "Invalid arguments",
                usage = format!("Usage: {} <field_name>", args[0]),
                available_fields = "name, is_vip, age"
            ));
        }

        let field_name = &args[1];

        println!("🚀 Querying field: {}", field_name);

        let db = setup_db().await?;
        setup_table(&db).await?;

        query_field(&db, field_name).await?;

        // Cleanup
        let _ = db.execute(&surreal_expr!("DELETE demo_user")).await;

        Ok(())
    }
    .await;

    if let Err(e) = result {
        use std::process::Termination;
        e.report();
    }
}
