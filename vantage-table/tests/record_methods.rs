use serde::{Deserialize, Serialize};
use vantage_dataset::prelude::*;
use vantage_table::prelude::MockTableSource;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
struct TestUser {
    id: Option<String>,
    name: String,
    email: String,
    active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
struct VipUser {
    id: Option<String>,
    name: String,
    email: String,
    vip_level: Option<String>,
}

#[tokio::test]
async fn test_iter_records_modify_and_save() {
    let mock = MockTableSource::new().with_data(
        "users",
        vec![
            serde_json::json!({"id": "1", "name": "Alice", "email": "alice@test.com", "active": false}),
            serde_json::json!({"id": "2", "name": "Bob", "email": "bob@test.com", "active": false}),
        ],
    );

    let table =
        Table::<MockTableSource, EmptyEntity>::new("users", mock.await).into_entity::<TestUser>();
    for mut record in table.list_entities().await.unwrap() {
        record.active = true;
        // MockTableSource now supports save, so this should succeed
        record.save().await.unwrap();
    }

    // Verify that modifications persisted
    let records = table.list_entities().await.unwrap();
    for record in records {
        assert!(record.active); // Should now be true after save
    }
}

#[tokio::test]
async fn test_get_some_record_modify_workflow() {
    let mock = MockTableSource::new().with_data(
        "users",
        vec![
            serde_json::json!({"id": "1", "name": "Alice", "email": "alice@test.com", "active": false}),
        ],
    );

    let table =
        Table::<MockTableSource, EmptyEntity>::new("users", mock.await).into_entity::<TestUser>();
    let mut record = table.get_some_entity().await.unwrap().unwrap();

    // Verify initial state
    assert_eq!(record.name, "Alice");
    assert_eq!(record.email, "alice@test.com");
    assert!(!record.active);
    assert_eq!(record.id(), "1");

    // Modify the record
    record.name = "Alice Updated".to_string();
    record.email = "alice.new@test.com".to_string();
    record.active = true;

    // Verify modifications
    assert_eq!(record.name, "Alice Updated");
    assert_eq!(record.email, "alice.new@test.com");
    assert!(record.active);
    assert_eq!(record.id(), "1"); // ID should remain unchanged

    // Save the record (should now succeed with MockTableSource)
    record.save().await.unwrap();

    // Verify changes persisted by fetching the record again
    let updated_record = table.get_some_entity().await.unwrap().unwrap();
    assert_eq!(updated_record.name, "Alice Updated");
    assert_eq!(updated_record.email, "alice.new@test.com");
    assert!(updated_record.active);
    assert_eq!(updated_record.id(), "1"); // ID should remain unchanged
}

#[tokio::test]
async fn test_record_type_conversion_workflow() {
    let mock = MockTableSource::new().with_data(
        "users",
        vec![serde_json::json!({"id": "1", "name": "Alice", "email": "alice@test.com", "active": true})],
    );

    let table =
        Table::<MockTableSource, EmptyEntity>::new("users", mock.await).into_entity::<TestUser>();
    let record = table.get_some_entity().await.unwrap().unwrap();

    // Convert TestUser record to VipUser (accessing the underlying entity)
    let vip_user = VipUser {
        id: record.id.clone(),
        name: record.name.clone(),
        email: record.email.clone(),
        vip_level: None,
    };

    // Verify the conversion worked and preserved data
    assert_eq!(vip_user.id, record.id);
    assert_eq!(vip_user.name, "Alice");
    assert_eq!(vip_user.email, "alice@test.com");
    // New field should have default value
    assert_eq!(vip_user.vip_level, None);
}

#[tokio::test]
async fn test_bulk_record_processing() {
    let mock = MockTableSource::new().with_data(
        "users",
        vec![
            serde_json::json!({"id": "1", "name": "Alice", "email": "alice@old.com", "active": false}),
            serde_json::json!({"id": "2", "name": "Bob", "email": "bob@old.com", "active": true}),
            serde_json::json!({"id": "3", "name": "Charlie", "email": "charlie@old.com", "active": false}),
        ],
    );

    let table =
        Table::<MockTableSource, EmptyEntity>::new("users", mock.await).into_entity::<TestUser>();
    let mut records = table.list_entities().await.unwrap();

    // Bulk operation: activate inactive users and update their emails
    let mut modified_count = 0;
    for record in &mut records {
        if !record.active {
            record.active = true;
            record.email = record.email.replace("@old.com", "@new.com");
            modified_count += 1;
        }
    }

    // Should have modified Alice and Charlie (2 users)
    assert_eq!(modified_count, 2);

    // Verify specific changes
    let alice = records.iter().find(|r| r.name == "Alice").unwrap();
    assert!(alice.active);
    assert_eq!(alice.email, "alice@new.com");

    let bob = records.iter().find(|r| r.name == "Bob").unwrap();
    assert!(bob.active); // Was already active
    assert_eq!(bob.email, "bob@old.com"); // Should remain unchanged

    let charlie = records.iter().find(|r| r.name == "Charlie").unwrap();
    assert!(charlie.active);
    assert_eq!(charlie.email, "charlie@new.com");

    // Attempt to save modified records
    let inactive_count_before = records.iter().filter(|r| !r.active).count();
    assert_eq!(inactive_count_before, 0); // All should be active now

    // Save all records (should now succeed with MockTableSource)
    for record in &records {
        record.save().await.unwrap();
    }

    // Verify changes persisted by fetching records again
    let updated_records = table.list_entities().await.unwrap();
    let alice = updated_records.iter().find(|r| r.name == "Alice").unwrap();
    assert!(alice.active);
    assert_eq!(alice.email, "alice@new.com");

    let charlie = updated_records
        .iter()
        .find(|r| r.name == "Charlie")
        .unwrap();
    assert!(charlie.active);
    assert_eq!(charlie.email, "charlie@new.com");

    let bob = updated_records.iter().find(|r| r.name == "Bob").unwrap();
    assert!(bob.active);
    assert_eq!(bob.email, "bob@old.com"); // Should remain unchanged
}

#[tokio::test]
async fn test_empty_table_record_methods() {
    let mock = MockTableSource::new().with_data("users", vec![]);
    let table =
        Table::<MockTableSource, EmptyEntity>::new("users", mock.await).into_entity::<TestUser>();

    // list_entities should return empty vector
    let records = table.list_entities().await.unwrap();
    assert_eq!(records.len(), 0);

    // get_entity should return None
    let record = table.get_some_entity().await.unwrap();
    assert!(record.is_none());
}
