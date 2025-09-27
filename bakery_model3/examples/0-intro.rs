use anyhow::Result;
use serde::{Deserialize, Serialize};

use bakery_model3::*;
use vantage_expressions::AssociatedQueryable;
use vantage_surrealdb::prelude::*;
use vantage_table::prelude::*;

async fn create_bootstrap_db() -> Result<()> {
    // Run this once for demos to work:
    //  > ./start.sh && ./ingress.sh (from vantage-surrealdb/scripts)
    //
    bakery_model3::connect_surrealdb().await?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    create_bootstrap_db().await?;

    let set_of_clients = Client::table();

    println!("-[ get entity values out of any table ]------------------------------------");
    // Regardless of DataSource - you can get all clients like this
    for client in set_of_clients.get().await? {
        println!("email: {}, client: {}", client.email, client.name);
    }

    println!("-[ vendor-specific select modification ]------------------------------------");
    // Since table is using SurrealDB as a source, we can get SurrealSelect,
    // or rather it's associated version, to tweak query before execution.
    let select = set_of_clients.select_surreal().with_limit(Some(1), Some(0));

    for client in select.get().await? {
        println!("email: {}, client: {}", client.email, client.name);
    }

    println!("-[ adding conditions ]------------------------------------");
    // using table["name"] for field referencing .eq() for conditions
    let paying_clients = set_of_clients
        .clone()
        .with_condition(set_of_clients["is_paying_client"].eq(true));

    for client in paying_clients.get().await? {
        println!(
            "client: {}, is_paying: {}",
            client.name, client.is_paying_client
        );
    }

    println!("-[ calculating count, implicit type ]-----------------------");
    // Generate i64 count() from Table<SurrealDB, Client> and execute it:
    let count_result = paying_clients.surreal_count().get().await?;
    println!("Count of paying clients: {}", count_result);

    println!("-[ change Doc Brown into non-paying ]-----------------------");
    set_of_clients
        .clone()
        .with_condition(set_of_clients["name"].eq("Doc Brown #VIP"))
        .map(|mut c: Client| {
            c.is_paying_client = false;
            c
        })
        .await?;

    // Generate i64 count() from Table<SurrealDB, Client> and execute it:
    let count_result = paying_clients.surreal_count().get().await?;
    println!("Count of paying clients: {}", count_result);

    println!("-[ add #VIP to all paying clients' names ]-----------------------");
    // Use map to transform paying clients by adding #VIP suffix to their names
    paying_clients
        .clone()
        .map(|mut client: Client| {
            if !client.name.contains("#VIP") {
                client.name = format!("{} #VIP", client.name);
            }
            client
        })
        .await?;

    println!("Updated paying clients with #VIP suffix");

    // Show the updated paying clients
    for client in paying_clients.get().await? {
        println!(
            "VIP client: {}, is_paying: {}",
            client.name, client.is_paying_client
        );
    }

    /////////////////////////////////////////////////////////////////////////////////////////
    println!("-------------------------------------------------------------------------------");
    /////////////////////////////////////////////////////////////////////////////////////////

    // TODO: Uncomment when relationships are implemented in 0.3
    // Traverse relationships to create order set:
    // let orders = paying_clients.ref_orders();

    // Lets pay attention to the type here:
    //  set_of_clients = Table<SurrealDB, Client>
    //  paying_clients = Table<SurrealDB, Client>
    //  orders         = Table<SurrealDB, Order>

    // TODO: Uncomment when custom methods and relationships are implemented
    // Execute my custom method on Table<SurrealDB, Order> from bakery_model3/src/order.rs:
    // let report = orders.generate_report().await?;
    // println!("Report:\n{}", report);

    // Using this method is safe, because it is unit-tested.

    /////////////////////////////////////////////////////////////////////////////////////////
    println!("-------------------------------------------------------------------------------");
    /////////////////////////////////////////////////////////////////////////////////////////

    // Queries are built by understanding which fields are needed. Lets define a new entity
    // type:
    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    struct MiniClient {
        name: String,
        email: String,
    }
    impl Entity for MiniClient {}

    // TODO: Uncomment when get_some_as method is implemented
    // Load a single client by executing a query like SELECT name, email FROM .....
    // let Some(mini_client) = set_of_clients.get_some_as::<MiniClient>().await? else {
    //     panic!("No client found");
    // };
    // println!("data = {:?}", &mini_client);
    // println!(
    //     "MiniClient query: {}",
    //     set_of_clients
    //         .get_select_query_for_struct(MiniClient::default())
    //         .preview()
    // );

    // MegaClient defines metadata access - SurrealDB allows embedded documents
    #[derive(Clone, Debug, Serialize, Deserialize, Default)]
    struct MegaClient {
        name: String,
        email: String,
        is_paying_client: bool,
        metadata: Option<serde_json::Value>,
    }
    impl Entity for MegaClient {}

    // TODO: Uncomment when advanced querying is implemented
    // The code is almost identical to the code above, but the query is more complex.
    // let Some(mega_client) = set_of_clients.get_some_as::<MegaClient>().await? else {
    //     panic!("No client found");
    // };
    // println!("data = {:?}", &mega_client);
    // println!(
    //     "MegaClient query: {}",
    //     set_of_clients
    //         .get_select_query_for_struct(MegaClient::default())
    //         .preview()
    // );

    // To continue learning, visit: <https://romaninsh.github.io/vantage>, Ok?
    Ok(())
}
