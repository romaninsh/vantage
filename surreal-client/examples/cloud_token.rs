//! Throwaway spike: connect to a SurrealDB Cloud instance with a brokered
//! access token (JWT) and run a query. Proves the wss/TLS + `authenticate`
//! path and doubles as an ad-hoc SurrealQL runner for that instance.
//! Run: `SURREAL_TOKEN=<jwt> SURREAL_QUERY='INFO FOR DB' \
//!       cargo run -p surreal-client --example cloud_token`

use surreal_client::SurrealConnection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("SURREAL_TOKEN").expect("set SURREAL_TOKEN");
    let host = std::env::var("SURREAL_HOST").unwrap_or_else(|_| {
        "wss://close-wasp-06fmkfm5uhq2f77ih987dcdac4.aws-euw1.surreal.cloud".into()
    });
    let query = std::env::var("SURREAL_QUERY")
        .unwrap_or_else(|_| "USE NS main DB `vantage-leads`; INFO FOR DB;".into());

    println!("connecting to {host} ...");
    let client = SurrealConnection::dsn(&host)?
        .auth_token(token)
        .version_check(false)
        .connect()
        .await?;
    println!(
        "connected. server version = {:?}\n",
        client.version().await?
    );

    let result = client.query(&query, None).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);

    Ok(())
}
