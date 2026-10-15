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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Person {
    id: Option<String>,
    name: String,
    age: i32,
}

fn person(id: &str, name: &str, age: i32) -> Person {
    Person {
        id: Some(id.to_string()),
        name: name.to_string(),
        age,
    }
}

async fn users(rows: Vec<serde_json::Value>) -> Table<MockTableSource, TestUser> {
    let mock = MockTableSource::new().with_data("users", rows).await;
    Table::<MockTableSource, EmptyEntity>::new("users", mock).into_entity::<TestUser>()
}

async fn people(rows: Vec<serde_json::Value>) -> Table<MockTableSource, Person> {
    let mock = MockTableSource::new().with_data("people", rows).await;
    Table::<MockTableSource, EmptyEntity>::new("people", mock).into_entity::<Person>()
}

#[tokio::test]
async fn test_active_entity_delete() {
    let mock = MockTableSource::new().with_data(
        "users",
        vec![
            serde_json::json!({"id": "1", "name": "Alice", "email": "alice@test.com", "active": true}),
            serde_json::json!({"id": "2", "name": "Bob", "email": "bob@test.com", "active": true}),
        ],
    );

    let table =
        Table::<MockTableSource, EmptyEntity>::new("users", mock.await).into_entity::<TestUser>();

    let alice = table.get_entity("1").await.unwrap().unwrap();
    alice.delete().await.unwrap();

    let missing = table.get_entity("1").await.unwrap();
    assert!(missing.is_none());

    // Other rows untouched.
    let bob = table.get_entity("2").await.unwrap();
    assert!(bob.is_some());
}

#[tokio::test]
async fn active_entity_reload_fails_when_row_is_gone() {
    let table = users(vec![
        serde_json::json!({"id": "1", "name": "Alice", "email": "a@test.com", "active": false}),
    ])
    .await;
    let mut alice = table.get_entity("1").await.unwrap().unwrap();

    WritableValueSet::delete(&table, "1").await.unwrap();

    assert!(alice.reload().await.is_err());
}

#[tokio::test]
async fn active_entity_reload_picks_up_an_external_write() {
    let table = users(vec![
        serde_json::json!({"id": "1", "name": "Alice", "email": "a@test.com", "active": false}),
    ])
    .await;
    let mut alice = table.get_entity("1").await.unwrap().unwrap();
    assert!(!alice.active);

    table
        .replace(
            "1",
            &TestUser {
                id: Some("1".to_string()),
                name: "Alice".into(),
                email: "a@test.com".into(),
                active: true,
            },
        )
        .await
        .unwrap();
    assert!(!alice.active, "the in-memory copy is stale until reloaded");

    alice.reload().await.unwrap();
    assert!(alice.active);
}

#[tokio::test]
async fn saving_every_listed_entity_persists_each_modification() {
    let table = users(vec![
        serde_json::json!({"id": "1", "name": "Alice", "email": "a@test.com", "active": false}),
        serde_json::json!({"id": "2", "name": "Bob", "email": "b@test.com", "active": false}),
    ])
    .await;
    for mut entity in table.list_entities().await.unwrap() {
        entity.active = true;
        entity.save().await.unwrap();
    }
    for entity in table.list_entities().await.unwrap() {
        assert!(entity.active, "{}", entity.name);
    }
}

#[tokio::test]
async fn get_entity_returns_the_row_by_id_and_none_for_a_missing_one() {
    let table = users(vec![
        serde_json::json!({"id": "1", "name": "Alice", "email": "a@test.com", "active": false}),
        serde_json::json!({"id": "2", "name": "Bob", "email": "b@test.com", "active": true}),
    ])
    .await;
    let alice = table.get_entity("1").await.unwrap().unwrap();
    assert_eq!(alice.name, "Alice");
    assert_eq!(alice.id(), "1");
    assert!(table.get_entity("999").await.unwrap().is_none());
}

#[tokio::test]
async fn a_modified_entity_is_saved_under_its_id() {
    let table = users(vec![
        serde_json::json!({"id": "1", "name": "Alice", "email": "a@test.com", "active": false}),
    ])
    .await;
    let mut record = table.get_some_entity().await.unwrap().unwrap();
    assert_eq!(record.id(), "1");
    record.name = "Alice Updated".to_string();
    record.active = true;
    record.save().await.unwrap();

    let updated = table.get_entity("1").await.unwrap().unwrap();
    assert_eq!(updated.name, "Alice Updated");
    assert!(updated.active);
    assert_eq!(updated.id(), "1");
}

// --- the typed dataset surface -----------------------------------------------

#[tokio::test]
async fn list_get_and_get_some_return_typed_entities_by_id() {
    let table = people(vec![
        serde_json::json!({"id": "1", "name": "Alice", "age": 30}),
        serde_json::json!({"id": "2", "name": "Bob", "age": 25}),
    ])
    .await;

    let all = table.list().await.unwrap();
    assert_eq!(all.keys().collect::<Vec<_>>(), ["1", "2"]);
    assert_eq!(all["1"], person("1", "Alice", 30));

    assert_eq!(table.get("2").await.unwrap(), Some(person("2", "Bob", 25)));
    assert_eq!(table.get("999").await.unwrap(), None);

    let (id, first) = table.get_some().await.unwrap().unwrap();
    assert_eq!(id, "1");
    assert_eq!(first.name, "Alice");
}

#[tokio::test]
async fn get_some_on_an_empty_table_is_none() {
    let table = people(vec![]).await;
    assert!(table.get_some().await.unwrap().is_none());
    assert!(table.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_row_that_does_not_fit_the_entity_is_a_read_error() {
    let table = people(vec![serde_json::json!({"id": "1", "name": "Alice"})]).await;
    assert!(table.list().await.is_err());
    assert!(table.get("1").await.is_err());
    assert!(table.get_some().await.is_err());
}

#[tokio::test]
async fn streamed_entities_are_the_listed_ones_in_order() {
    let table = people(vec![
        serde_json::json!({"id": "1", "name": "Alice", "age": 30}),
        serde_json::json!({"id": "2", "name": "Bob", "age": 25}),
    ])
    .await;
    let mut stream = table.stream();
    let mut seen = Vec::new();
    while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
        seen.push(item.unwrap());
    }
    assert_eq!(
        seen,
        [
            ("1".to_string(), person("1", "Alice", 30)),
            ("2".to_string(), person("2", "Bob", 25)),
        ]
    );
}

#[tokio::test]
async fn streaming_a_row_that_does_not_fit_the_entity_yields_an_error() {
    let table = people(vec![serde_json::json!({"id": "1", "name": "Alice"})]).await;
    let mut stream = table.stream();
    let first = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
    assert!(first.unwrap().is_err());
}

#[tokio::test]
async fn insert_stores_a_new_entity_and_returns_it_as_stored() {
    let table = people(vec![]).await;
    let stored = table
        .insert("3", &person("3", "Charlie", 35))
        .await
        .unwrap();
    assert_eq!(stored, person("3", "Charlie", 35));
    assert_eq!(
        table.get("3").await.unwrap(),
        Some(person("3", "Charlie", 35))
    );
}

#[tokio::test]
async fn insert_of_an_existing_id_returns_the_stored_entity_untouched() {
    let table = people(vec![
        serde_json::json!({"id": "1", "name": "Alice", "age": 30}),
    ])
    .await;
    let stored = table.insert("1", &person("1", "David", 40)).await.unwrap();
    assert_eq!(stored.name, "Alice");
    assert_eq!(table.get("1").await.unwrap().unwrap().name, "Alice");
}

#[tokio::test]
async fn replace_overwrites_an_entity_and_creates_a_missing_one() {
    let table = people(vec![
        serde_json::json!({"id": "2", "name": "Bob", "age": 25}),
    ])
    .await;
    let replaced = table
        .replace("2", &person("2", "Bob Updated", 26))
        .await
        .unwrap();
    assert_eq!(replaced, person("2", "Bob Updated", 26));
    table.replace("4", &person("4", "Eve", 28)).await.unwrap();
    assert_eq!(table.get("4").await.unwrap(), Some(person("4", "Eve", 28)));
}

#[tokio::test]
async fn patch_changes_the_fields_given_and_returns_the_whole_stored_entity() {
    let table = people(vec![
        serde_json::json!({"id": "1", "name": "Alice", "age": 30}),
    ])
    .await;
    let patched = table
        .patch("1", &person("1", "Alice Updated", 30))
        .await
        .unwrap();
    assert_eq!(patched, person("1", "Alice Updated", 30));
}

#[tokio::test]
async fn patch_of_a_missing_entity_is_not_found() {
    let table = people(vec![]).await;
    let err = table
        .patch("999", &person("999", "Ghost", 50))
        .await
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");
}

#[tokio::test]
async fn delete_and_delete_all_remove_entities_and_a_missing_id_is_fine() {
    let table = people(vec![
        serde_json::json!({"id": "1", "name": "Alice", "age": 30}),
        serde_json::json!({"id": "2", "name": "Bob", "age": 25}),
    ])
    .await;
    WritableValueSet::delete(&table, "2").await.unwrap();
    assert!(table.get("2").await.unwrap().is_none());
    WritableValueSet::delete(&table, "999").await.unwrap();
    WritableValueSet::delete_all(&table).await.unwrap();
    assert!(table.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn insert_return_id_makes_a_fresh_id_for_each_call() {
    let table = people(vec![]).await;
    let entity = Person {
        id: None,
        name: "Alice".into(),
        age: 30,
    };
    let first = table.insert_return_id(&entity).await.unwrap();
    let second = table.insert_return_id(&entity).await.unwrap();
    assert!(!first.is_empty());
    assert_ne!(first, second);
    assert_eq!(table.list().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_empty_table_record_methods() {
    let mock = MockTableSource::new().with_data("users", vec![]);
    let table =
        Table::<MockTableSource, EmptyEntity>::new("users", mock.await).into_entity::<TestUser>();

    // list_entities should return empty vector
    let records = table.list_entities().await.unwrap();
    assert_eq!(records.len(), 0);

    // get_some_entity should return None
    let record = table.get_some_entity().await.unwrap();
    assert!(record.is_none());

    // Test get_entity or new_entity pattern
    let mut user = table
        .get_entity("new_user")
        .await
        .unwrap()
        .unwrap_or_else(|| {
            table.new_entity(
                "new_user".to_string(),
                TestUser {
                    id: Some("new_user".to_string()),
                    name: "New User".to_string(),
                    email: "new@example.com".to_string(),
                    active: false,
                },
            )
        });

    // Modify the new entity
    user.active = true;
    user.email = "updated@example.com".to_string();

    // Save the new entity
    user.save().await.unwrap();

    // Verify it was created and saved
    let saved_user = table.get_entity("new_user").await.unwrap().unwrap();
    assert_eq!(saved_user.name, "New User");
    assert_eq!(saved_user.email, "updated@example.com");
    assert!(saved_user.active);
    assert_eq!(saved_user.id(), "new_user");
}
