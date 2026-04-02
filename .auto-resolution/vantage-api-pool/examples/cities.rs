use std::sync::{Arc, OnceLock};

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use vantage_api_pool::{AwwPool, PoolApi};
use vantage_cli_util::print_table;
use vantage_dataset::prelude::ReadableDataSet;
use vantage_table::table::Table;

// --- Static pool data source ---

static POOL: OnceLock<PoolApi> = OnceLock::new();

pub fn set_pool(pool: PoolApi) {
    POOL.set(pool).expect("Pool already initialized");
}

pub fn pool() -> PoolApi {
    POOL.get().expect("Pool not initialized").clone()
}

// --- Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Country {
    pub name: String,
}

impl Country {
    pub fn api_table() -> Table<PoolApi, Country> {
        Table::new("countries", pool())
            .with_id_column("name")
    }

    pub fn ref_cities(&self) -> Table<PoolApi, City> {
        City::for_country(&self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct City {
    pub name: String,
    pub population: i64,
}

impl City {
    pub fn for_country(country: &str) -> Table<PoolApi, City> {
        let endpoint = format!("countries/{}/cities", urlencoding::encode(country));
        Table::new(&endpoint, pool())
            .with_id_column("name")
            .with_column_of::<i64>("population")
    }
}

// --- Main ---

#[tokio::main]
async fn main() -> Result<()> {
    // Create pool with auth
    let aww_pool = Arc::new(
        AwwPool::new(3, None, false, "http://127.0.0.1:3030".to_string()).with_auth_callback(
            1,
            || async {
                let client = Client::new();
                let res = client.post("http://127.0.0.1:3030/auth").send().await?;
                let body: Value = res.json().await?;
                let token = body["token"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("No token in response"))?;
                Ok(token.to_string())
            },
            |mut req, token| {
                req.headers_mut().insert(
                    "Authorization",
                    format!("Bearer {}", token).parse().unwrap(),
                );
                req
            },
        ),
    );

    // Health check
    let res = aww_pool.get("/health").await?;
    if !res.status().is_success() {
        anyhow::bail!("Start test-api-server first: {}", res.status());
    }

    set_pool(PoolApi::new(aww_pool));

    // List all countries (auto-paginates through all pages)
    let countries = Country::api_table();
    println!("Countries:");
    print_table(&countries).await?;

    // Load a country and traverse to its cities
    let argentina: Country = countries.get("Argentina").await?;
    let cities = argentina.ref_cities();
    println!("\nCities in {}:", argentina.name);
    print_table(&cities).await?;

    // Load a specific city
    let rosario: City = cities.get("Rosario").await?;
    println!("\nRosario population: {}", rosario.population);

    Ok(())
}
