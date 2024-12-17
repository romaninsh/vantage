use anyhow::Result;
use bakery_model::*;

use vantage::prelude::*;

// async fn create_bootstrap_db() -> Result<()> {
//     let client = POSTGRESS.get().unwrap().client();

//     bakery_model::create_tables().await;

//     client
//         .execute(
//             "
//         CREATE TABLE IF NOT EXISTS table1 (
//                     id SERIAL PRIMARY KEY,
//                     name TEXT NOT NULL
//                 );
//                 ",
//             &[],
//         )
//         .await?;
//     client
//         .execute(
//             "
//                 CREATE TABLE IF NOT EXISTS table2 (
//                     id SERIAL PRIMARY KEY,
//                     data TEXT NOT NULL
//                 );",
//             &[],
//         )
//         .await?;

//     client
//         .execute(
//             "
//             INSERT INTO table1 (name) VALUES ('Alice'), ('Bob');
//             ",
//             &[],
//         )
//         .await?;
//     client
//         .execute(
//             "
//             INSERT INTO table2 (data) VALUES ('Data1'), ('Data2');
//             ",
//             &[],
//         )
//         .await?;

//     Ok(())
// }

// async fn init() {
//     connect_postgres()
//         .await
//         .context("starting postgres")
//         .unwrap();
// }
async fn create_bootstrap_db() -> Result<()> {
    // Connect to postgress and store client statically
    bakery_model::connect_postgres().await?;

    // Get the postgres client for batch execution
    let vantage_client = bakery_model::postgres();
    let client = vantage_client.pool;

    // Read the schema from the file and execute it
    let schema = tokio::fs::read_to_string("schema-pg.sql").await?;
    sqlx::raw_sql(&schema).execute(&*client).await?;

    Ok(())
}

// TODO: get rid of testcontainers, yukk.

// #[tokio::test]
// async fn should_create_bucket() {
//     init().await;

//     let postgres = postgres();

//     let res = postgres
//         .query_raw(
//             &Query::new()
//                 .set_table("table1", None)
//                 .add_column_field("name"),
//         )
//         .await
//         .unwrap();

//     assert_eq!(res, vec![json!({"name": "Alice"}), json!({"name": "Bob"}),]);
// }

#[tokio::test]
async fn test_bakery() -> Result<()> {
    create_bootstrap_db().await?;

    println!("In this example, we will be interracting with the records and testing conditions");
    let products = Product::table();

    println!(
        "We are starting with {} products",
        products.count().get_one_untyped().await?
    );

    println!("");
    println!("Adding a single new product");
    let id = products
        .insert(Product {
            name: "Nuclear Sandwich".to_string(),
            calories: 100,
            bakery_id: 1,
            price: 120,
        })
        .await?;

    println!(
        "After adding \"Nuclear Sandwich\" (id={}) we are left with {} products",
        id.unwrap(),
        products.count().get_one_untyped().await?
    );

    Ok(())
}
