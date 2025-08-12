use surreal_client::{SurrealClient, SurrealConnection};
use vantage_expressions::DataSource;
use vantage_surrealdb::operation::Expressive;
use vantage_surrealdb::select::SurrealSelect;
use vantage_surrealdb::SurrealDB;

pub async fn surreal_connect() -> Result<SurrealDB, surreal_client::SurrealError> {
    let db = SurrealConnection::dsn("http://root:root@localhost:8000")?
        .connect()
        .await?;

    let db = SurrealDB::new(db);
    Ok(db)
}

pub fn get_batches() -> SurrealSelect {
    SurrealSelect::new()
        .with_source("batches")
        .with_field("id")
        .with_field("name")
        .with_field("golf_course")
        .with_field("total_tags")
        .with_field("created")
}

pub async fn get_batches_query() {
    let db = surreal_connect().await.unwrap();

    let batches = db.execute(get_batches().expr()).await;
}
