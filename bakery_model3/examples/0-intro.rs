use serde::{Deserialize, Serialize};

use bakery_model3::*;
use vantage_core::{error, util::error::Context, Result};
use vantage_expressions::AssociatedQueryable;
use vantage_surrealdb::prelude::*;
use vantage_table::record::RecordTable;

async fn create_bootstrap_db() -> Result<()> {
    // Run this once for demos to work:
    //  > ./start.sh && ./ingress.sh (from vantage-surrealdb/scripts)
    //
    bakery_model3::connect_surrealdb()
        .await
        .with_context(|| error!("Failed to initialize database"))?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    create_bootstrap_db().await?;

    let set_of_clients = Client::table(surrealdb());

    println!("-[ get entity values out of any table ]------------------------------------");
    // Regardless of DataSource - you can get all clients like this
    for client in set_of_clients
        .get()
        .await
        .with_context(|| error!("Failed to retrieve clients"))?
    {
        println!("email: {}, client: {}", client.email, client.name);
    }

    println!("-[ vendor-specific select modification ]------------------------------------");
    // Since table is using SurrealDB as a source, we can get SurrealSelect,
    // or rather it's associated version, to tweak query before execution.
    let select = set_of_clients.select_surreal().with_limit(Some(1), Some(0));

    for client in select
        .get()
        .await
        .with_context(|| error!("Failed to retrieve limited clients"))?
    {
        println!("email: {}, client: {}", client.email, client.name);
    }

    println!("-[ adding conditions ]------------------------------------");
    // using table["name"] for field referencing .eq() for conditions
    let paying_clients = set_of_clients
        .clone()
        .with_condition(set_of_clients["is_paying_client"].eq(true));

    for client in paying_clients
        .get()
        .await
        .with_context(|| error!("Failed to retrieve paying clients"))?
    {
        println!(
            "client: {}, is_paying: {}",
            client.name, client.is_paying_client
        );
    }

    println!("-[ calculating count, implicit type ]-----------------------");
    // Generate i64 count() from Table<SurrealDB, Client> and execute it:
    let count_result = paying_clients
        .surreal_count()
        .get()
        .await
        .with_context(|| error!("Failed to count paying clients"))?;
    println!("Count of paying clients: {}", count_result);

    println!("-[ change Doc Brown into non-paying ]-----------------------");
    set_of_clients
        .clone()
        .with_condition(set_of_clients["name"].eq("Doc Brown #VIP"))
        .map(|mut c: Client| {
            c.is_paying_client = false;
            c
        })
        .await
        .with_context(|| error!("Failed to update Doc Brown payment status"))?;

    // Generate i64 count() from Table<SurrealDB, Client> and execute it:
    let count_result = paying_clients
        .surreal_count()
        .get()
        .await
        .with_context(|| error!("Failed to count paying clients after update"))?;
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
        .await
        .with_context(|| error!("Failed to add VIP suffix to paying clients"))?;

    println!("Updated paying clients with #VIP suffix");

    // Show the updated paying clients
    for client in paying_clients
        .get()
        .await
        .with_context(|| error!("Failed to retrieve VIP clients"))?
    {
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

    println!("-[ Record functionality test ]------------------------------------");

    // Get clients with their IDs by explicitly selecting the id field
    let select_with_id = set_of_clients.select().with_field("id");
    let raw_results = select_with_id.get(set_of_clients.data_source()).await;

    let first_result = raw_results
        .first()
        .ok_or_else(|| error!("No clients found in database"))?;
    let json_output =
        serde_json::to_string_pretty(&serde_json::Value::Object(first_result.clone()))
            .with_context(|| error!("Failed to serialize client data to JSON"))?;
    println!("Raw client data with ID: {}", json_output);

    let id_value = first_result
        .get("id")
        .ok_or_else(|| error!("No 'id' field found in record"))?;
    let id_str = id_value
        .as_str()
        .ok_or_else(|| error!("ID field is not a string"))?;
    println!("Attempting to get record with ID: {}", id_str);

    // Record type is now: Record<'_, Client, Table<SurrealDB, Client>>
    let mut client_record = set_of_clients
        .get_record(id_str)
        .await
        .with_context(|| error!("Failed to fetch record", id = id_str))?
        .ok_or_else(|| error!("Record not found", id = id_str))?;

    println!("✓ Successfully retrieved client record:");
    println!("  ID: {}", client_record.id());
    println!("  Name: {}", client_record.name);
    println!("  Email: {}", client_record.email);
    println!("  Is paying: {}", client_record.is_paying_client);
    println!("  Contact: {}", client_record.contact_details);

    // Modify the record through DerefMut - only update contact_details to avoid unique constraints
    let old_contact = client_record.contact_details.clone();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .with_context(|| error!("Failed to get system time"))?
        .as_secs();
    client_record.contact_details = format!("Updated via Record API at {}", timestamp);

    // Save changes back to database
    client_record
        .save()
        .await
        .with_context(|| error!("Failed to save client record", id = id_str))?;
    println!("✓ Updated and saved client record");
    println!("  Old contact: {}", old_contact);
    println!("  New contact: {}", client_record.contact_details);

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
