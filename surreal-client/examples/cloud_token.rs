//! Throwaway spike: connect to a SurrealDB Cloud instance with a brokered
//! access token (JWT), proving the wss/TLS + `authenticate` path.
//! Run: `SURREAL_TOKEN=<jwt> cargo run -p surreal-client --example cloud_token`

use surreal_client::SurrealConnection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("SURREAL_TOKEN").expect("set SURREAL_TOKEN");
    let host = std::env::var("SURREAL_HOST")
        .unwrap_or_else(|_| "wss://close-wasp-06fmkfm5uhq2f77ih987dcdac4.aws-euw1.surreal.cloud".into());

    println!("connecting to {host} ...");
    let client = SurrealConnection::dsn(&host)?
        .auth_token(token)
        .version_check(false)
        .connect()
        .await?;

    println!("connected. server version = {:?}", client.version().await?);

    let info = client.query("INFO FOR ROOT", None).await?;
    println!("INFO FOR ROOT =\n{}", serde_json::to_string_pretty(&info)?);

    Ok(())
}
