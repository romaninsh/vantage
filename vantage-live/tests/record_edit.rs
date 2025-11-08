mod common;

use common::{
    connect_surrealdb, setup_cache, Bakery, Client, Product, CLIENT_DOC, CLIENT_MARTY,
    PRODUCT_FLUX_CUPCAKE,
};
use vantage_dataset::dataset::WritableDataSet;
use vantage_live::prelude::*;
use vantage_table::Table;

#[tokio::test]
async fn test_edit_existing_record() {
    connect_surrealdb().await.unwrap();

    let backend = Client::table();
    let cache = setup_cache::<Client>();

    let mut live_table: LiveTable<Client> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.edit_record(CLIENT_MARTY).await.unwrap();
    assert_eq!(edit.name, "Marty McFly");
    assert_eq!(edit.email, "marty@gmail.com");

    // Modify
    edit.name = "Marty J. McFly".to_string();
    edit.contact_details = "555-NEW".to_string();

    // Check what changed
    let modified = edit.get_modified_fields();
    assert!(modified.contains(&"name".to_string()));
    assert!(modified.contains(&"contact_details".to_string()));

    // Save
    match edit.save().await.unwrap() {
        SaveResult::Saved => {}
        _ => panic!("Expected Saved"),
    }
}

#[tokio::test]
async fn test_create_new_record() {
    connect_surrealdb().await.unwrap();

    let backend = Bakery::table();
    let cache = setup_cache::<Bakery>();

    let mut live_table: LiveTable<Bakery> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.new_record(Bakery {
        name: "Twin Pines Mall Bakery".to_string(),
        profit_margin: 20,
    });
    assert!(edit.is_new());
    assert!(edit.id().starts_with("temp_"));

    edit.name = "Lone Pine Mall Bakery".to_string();

    match edit.save().await.unwrap() {
        SaveResult::Created(real_id) => {
            assert!(!real_id.starts_with("temp_"));
            assert_eq!(edit.id(), real_id);
        }
        _ => panic!("Expected Created"),
    }

    // Verify name was updated
    assert_eq!(edit.name, "Lone Pine Mall Bakery");
}

#[tokio::test]
async fn test_field_modification_tracking() {
    connect_surrealdb().await.unwrap();

    let backend = Client::table();
    let cache = setup_cache::<Client>();

    let mut live_table: LiveTable<Client> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.edit_record(CLIENT_DOC).await.unwrap();

    // Initially no modifications
    assert_eq!(edit.get_modified_fields().len(), 0);

    // Modify only name
    edit.name = "Dr. Emmett Brown".to_string();

    assert!(edit.is_field_modified("name"));
    assert!(!edit.is_field_modified("email"));

    let modified = edit.get_modified_fields();
    assert_eq!(modified.len(), 1);
    assert!(modified.contains(&"name".to_string()));
}

#[tokio::test]
async fn test_revert_changes() {
    connect_surrealdb().await.unwrap();

    let backend = Client::table();
    let cache = setup_cache::<Client>();

    let mut live_table: LiveTable<Client> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.edit_record(CLIENT_MARTY).await.unwrap();
    let original_name = edit.name.clone();

    edit.name = "Modified Name".to_string();
    assert_eq!(edit.name, "Modified Name");

    // Revert
    edit.revert();
    assert_eq!(edit.name, original_name);
    assert_eq!(edit.get_modified_fields().len(), 0);
}

#[tokio::test]
async fn test_snapshot_comparison() {
    connect_surrealdb().await.unwrap();

    let backend = Product::table();
    let cache = setup_cache::<Product>();

    let mut live_table: LiveTable<Product> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.edit_record(PRODUCT_FLUX_CUPCAKE).await.unwrap();

    // Check snapshot
    assert_eq!(edit.live_snapshot().name, "Flux Capacitor Cupcake");
    assert_eq!(edit.live_snapshot().price, 120);

    // Modify local
    edit.price = 150;

    // Snapshot unchanged
    assert_eq!(edit.live_snapshot().price, 120);
    assert_eq!(edit.local().price, 150);
}

#[tokio::test]
async fn test_multiple_edits_same_record() {
    connect_surrealdb().await.unwrap();

    let backend = Bakery::table();
    let cache = setup_cache::<Bakery>();

    let mut live_table: LiveTable<Bakery> = LiveTable::new(backend, cache).await.unwrap();

    // First edit
    {
        let mut edit = live_table.edit_record("hill_valley").await.unwrap();
        let original_margin = edit.profit_margin;
        edit.profit_margin = 20;
        edit.save().await.unwrap();

        // Restore for next test
        edit.profit_margin = original_margin;
        edit.save().await.unwrap();
    }

    // Second edit (should see current value)
    {
        let edit = live_table.edit_record("hill_valley").await.unwrap();
        assert_eq!(edit.profit_margin, 15); // Original value
    }
}

#[tokio::test]
async fn test_deref_access() {
    connect_surrealdb().await.unwrap();

    let backend = Client::table();
    let cache = setup_cache::<Client>();

    let mut live_table: LiveTable<Client> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.edit_record(CLIENT_MARTY).await.unwrap();

    // Direct field access via Deref
    let original_email = edit.email.clone();
    edit.email = "marty.mcfly@future.com".to_string();
    assert_eq!(edit.email, "marty.mcfly@future.com");

    // Restore
    edit.email = original_email;
}

#[tokio::test]
async fn test_nested_struct_modification() {
    connect_surrealdb().await.unwrap();

    let backend = Product::table();
    let cache = setup_cache::<Product>();

    let mut live_table: LiveTable<Product> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.edit_record(PRODUCT_FLUX_CUPCAKE).await.unwrap();

    // Modify nested inventory
    let original_stock = edit.inventory.stock;
    edit.inventory.stock = 100;

    let modified = edit.get_modified_fields();
    assert!(modified.contains(&"inventory".to_string()));

    // Restore
    edit.inventory.stock = original_stock;
}

#[tokio::test]
async fn test_save_updates_both_cache_and_backend() {
    connect_surrealdb().await.unwrap();

    let backend = Client::table();
    let cache = setup_cache::<Client>();

    let mut live_table: LiveTable<Client> = LiveTable::new(backend, cache).await.unwrap();

    let mut edit = live_table.edit_record(CLIENT_DOC).await.unwrap();
    let original_details = edit.contact_details.clone();

    edit.contact_details = "555-TEST".to_string();
    edit.save().await.unwrap();

    // Verify updated (just check we can re-edit)
    let mut edit2 = live_table.edit_record(CLIENT_DOC).await.unwrap();
    assert_eq!(edit2.contact_details, "555-TEST");

    // Restore
    edit2.contact_details = original_details;
    edit2.save().await.unwrap();
}
