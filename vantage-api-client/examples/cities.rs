use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use vantage_api_client::RestApi;
use vantage_cli_util::print_table;
use vantage_dataset::prelude::ReadableDataSet;
use vantage_table::table::Table;

// --- Static API data source ---

static API: OnceLock<RestApi> = OnceLock::new();

pub fn set_api(rest_api: RestApi) {
    API.set(rest_api).expect("API already initialized");
}

pub fn api() -> RestApi {
    API.get().expect("API not initialized").clone()
}

// --- Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Country {
    pub name: String,
}

impl Country {
    pub fn api_table() -> Table<RestApi, Country> {
        Table::new("countries", api()).with_id_column("name")
    }

    pub fn ref_cities(&self) -> Table<RestApi, City> {
        City::for_country(&self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct City {
    pub name: String,
    pub population: i64,
}

impl City {
    pub fn for_country(country: &str) -> Table<RestApi, City> {
        let endpoint = format!("countries/{}/cities", urlencoding::encode(country));
        Table::new(&endpoint, api())
            .with_id_column("name")
            .with_column_of::<i64>("population")
    }
}

// --- Main ---

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Authenticate
    let client = reqwest::Client::new();
    let res = client.post("http://127.0.0.1:3030/auth").send().await?;
    let body: Value = res.json().await?;
    let token = body["token"].as_str().expect("No token in response");

    // Initialize static API
    set_api(RestApi::new("http://127.0.0.1:3030").with_auth(format!("Bearer {}", token)));

    // List countries
    let countries = Country::api_table();
    println!("Countries:");
    print_table(&countries).await?;

    // Load Argentina (on first page) → traverse to cities
    let country: Country = countries.get("Argentina").await?;
    let cities = country.ref_cities();
    println!("\nCities in {}:", country.name);
    print_table(&cities).await?;

    // Load a specific city entity
    let rosario: City = cities.get("Rosario").await?;
    println!("\nRosario population: {}", rosario.population);

    Ok(())
}
