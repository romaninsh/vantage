use surreal_client::SurrealConnection;
use vantage_surrealdb::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect to local SurrealDB instance using DSN
    let db = SurrealConnection::dsn("ws://root:root@localhost:8000/bakery/v1")?
        .connect()
        .await?;

    // Create Vantage data source
    let ds = SurrealDB::new(db);

    // Build query using SurrealSelect
    let select = SurrealSelect::new()
        .with_source("bakery")
        .with_field("name");

    // Execute with query builder
    let data = ds.get(select).await;
    println!("Bakery data: {:?}", data);

    Ok(())
}
