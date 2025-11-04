use anyhow::Result;
use vantage::prelude::*;

use bakery_model::*;
use serde::{Deserialize, Serialize};

async fn create_bootstrap_db() -> Result<()> {
    // Run this once for demos to work:
    //  > psql -d postgres -c "CREATE ROLE postgres WITH LOGIN SUPERUSER"
    //
    bakery_model::connect_postgres().await?;
    let vantage_client = bakery_model::postgres();
    let client = vantage_client.client();
    let schema = tokio::fs::read_to_string("schema-pg.sql").await?;
    sqlx::raw_sql(&schema).execute(client).await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    create_bootstrap_db().await?;

    // This example is explained in README.md <https://github.com/romaninsh/vantage>.
    //
    // Use a set of our clients as a type:
    let set_of_clients = Client::table();

    // As you would expect, you can iterate over clients easily.
    for client in set_of_clients.get().await? {
        println!("id: {}, client: {}", client.id, client.name);
    }

    /////////////////////////////////////////////////////////////////////////////////////////
    println!("-------------------------------------------------------------------------------");
    /////////////////////////////////////////////////////////////////////////////////////////

    // Create and apply conditions to create a new set:
    let condition = set_of_clients.is_paying_client().eq(&true);
    let paying_clients = set_of_clients.with_condition(condition);

    // Generate count() Query from Table<Postgres, Client> and execute it:
    println!(
        "Count of paying clients: {}",
        paying_clients.count().get_one_untyped().await?
    );

    /////////////////////////////////////////////////////////////////////////////////////////
    println!("-------------------------------------------------------------------------------");
    /////////////////////////////////////////////////////////////////////////////////////////

    // Traverse relationships to create order set:
    let orders = paying_clients.ref_orders();

    // Lets pay attention to the type here:
    //  set_of_cilents = Table<Postgres, Client>
    //  paying_clients = Table<Postgres, Client>
    //  orders         = Table<Postgres, Order>

    // Execute my custom method on Table<Postgres, Order> from bakery_model/src/order.rs:
    let report = orders.generate_report().await?;
    println!("Report:\n{}", report);

    // Using this method is safe, because it is unit-tested.

    /////////////////////////////////////////////////////////////////////////////////////////
    println!("-------------------------------------------------------------------------------");
    /////////////////////////////////////////////////////////////////////////////////////////

    // Queries are built by understanding which fields are needed. Lets define a new entity
    // type:
    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    struct MiniOrder {
        id: i64,
        client_id: i64,
    }
    impl Entity for MiniOrder {}

    // Load a single order by executing a query like SELECT id, client_id FROM .....
    let Some(mini_order) = orders.get_some_as::<MiniOrder>().await? else {
        panic!("No order found");
    };
    println!("data = {:?}", &mini_order);
    println!(
        "MiniOrder query: {}",
        orders
            .get_select_query_for_struct(MiniOrder::default())
            .preview()
    );

    // MegaOrder defines `client_name` and `total` - those are not physical fields, but rather
    // defined through expressions/subqueries from related tables.
    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    struct MegaOrder {
        id: i64,
        client_name: String,
        total: i64,
    }
    impl Entity for MegaOrder {}

    // The code is almost identical to the code above, but the query is more complex.
    let Some(mini_order) = orders.get_some_as::<MegaOrder>().await? else {
        panic!("No order found");
    };
    println!("data = {:?}", &mini_order);
    println!(
        "MegaOrder query: {}",
        orders
            .get_select_query_for_struct(MegaOrder::default())
            .preview()
    );

    // To continue learning, visit: <https://romaninsh.github.io/vantage>, Ok?
    Ok(())
}
