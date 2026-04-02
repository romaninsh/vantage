mod common;

use common::{connect_surrealdb, setup_cache, Bakery, Client, BAKERY_HILL_VALLEY, CLIENT_MARTY};
use vantage_dataset::dataset::ReadableDataSet;
use vantage_live::prelude::*;
use vantage_table::Table;

#[tokio::test]
async fn test_new_livetable_populates_cache() {
    connect_surrealdb().await.unwrap();

    let backend = Bakery::table();
    let cache = setup_cache::<Bakery>();

    // Create LiveTable (should populate cache from SurrealDB)
    let _live_table: LiveTable<Bakery> = LiveTable::new(backend, cache).await.unwrap();

    // Verify cache was populated with Hill Valley Bakery from v2.surql
    let cache2 = setup_cache::<Bakery>();
    let cached = cache2.get().await.unwrap();
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].name, "Hill Valley Bakery");
    assert_eq!(cached[0].profit_margin, 15);
}

#[tokio::test]
async fn test_read_from_cache() {
    connect_surrealdb().await.unwrap();

    let backend = Client::table();
    let cache = setup_cache::<Client>();

    let live_table: LiveTable<Client> = LiveTable::new(backend, cache).await.unwrap();

    // Read should hit cache (3 clients from v2.surql: marty, doc, biff)
    let clients = live_table.get().await.unwrap();
    assert_eq!(clients.len(), 3);

    // Check known clients exist
    let names: Vec<&str> = clients.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Marty McFly"));
    assert!(names.contains(&"Doc Brown"));
    assert!(names.contains(&"Biff Tannen"));
}

#[tokio::test]
async fn test_refresh_all_updates_cache() {
    connect_surrealdb().await.unwrap();

    let backend = Bakery::table();
    let cache = setup_cache::<Bakery>();

    let mut live_table: LiveTable<Bakery> = LiveTable::new(backend, cache).await.unwrap();

    // Initial cache has Hill Valley Bakery
    let cache2 = setup_cache::<Bakery>();
    let initial = cache2.get().await.unwrap();
    assert_eq!(initial.len(), 1);

    // Refresh (should still have same data)
    live_table.refresh_all().await.unwrap();

    let cache3 = setup_cache::<Bakery>();
    let refreshed = cache3.get().await.unwrap();
    assert_eq!(refreshed.len(), 1);
    assert_eq!(refreshed[0].name, "Hill Valley Bakery");
}

#[tokio::test]
async fn test_get_by_id() {
    connect_surrealdb().await.unwrap();

    let backend = Client::table();
    let cache = setup_cache::<Client>();

    let live_table: LiveTable<Client> = LiveTable::new(backend, cache).await.unwrap();

    // Get Marty McFly by ID
    let marty = live_table.get_id(CLIENT_MARTY).await.unwrap();
    assert_eq!(marty.name, "Marty McFly");
    assert_eq!(marty.email, "marty@gmail.com");
    assert!(marty.is_paying_client);
}
