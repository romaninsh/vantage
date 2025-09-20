use anyhow::Result;
use serde::{Deserialize, Serialize};

use bakery_model3::*;
use vantage_surrealdb::SurrealTableExt;
use vantage_table::Entity;

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

    // Client table represents remotely stored clients.

    for client in set_of_clients.surreal_get().await? {
        println!("email: {}, client: {}", client.email, client.name);
    }

    for client in set_of_clients.get().await? {
        println!("email: {}, client: {}", client.email, client.name);
    }

    /////////////////////////////////////////////////////////////////////////////////////////
    println!("-------------------------------------------------------------------------------");
    /////////////////////////////////////////////////////////////////////////////////////////

    // TODO: Uncomment when condition system is implemented in 0.3
    // Create and apply conditions to create a new set:
    // let condition = set_of_clients.is_paying_client().eq(&true);
    // let paying_clients = set_of_clients.with_condition(condition);

    // TODO: Uncomment when count() method is implemented in 0.3
    // Generate count() Query from Table<SurrealDB, Client> and execute it:
    // println!(
    //     "Count of paying clients: {}",
    //     paying_clients.count().get_one_untyped().await?
    // );

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
