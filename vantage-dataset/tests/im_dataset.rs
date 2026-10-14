use serde::{Deserialize, Serialize};

use vantage_dataset::{
    im::{ImDataSource, ImTable},
    traits::{InsertableDataSet, ReadableDataSet, WritableDataSet, WritableValueSet},
};

// Simple test entities with serde Record conversion
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
struct User {
    id: Option<String>,
    name: String,
    age: i32,
    active: bool,
}

/// An entity whose serialization always fails. Used to prove write paths surface
/// the error as a `Result` instead of panicking — a regression guard for the old
/// `IntoRecord` `.expect()`.
#[derive(Debug, Clone)]
struct Unserializable;

impl Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom(
            "serialization deliberately failed",
        ))
    }
}

impl<'de> Deserialize<'de> for Unserializable {
    fn deserialize<D: serde::Deserializer<'de>>(_deserializer: D) -> Result<Self, D::Error> {
        Ok(Unserializable)
    }
}

#[tokio::test]
async fn test_readable_dataset() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    // Test empty list
    let result = table.list().await.unwrap();
    assert_eq!(result.len(), 0);

    // Test get non-existent record
    let result = table.get("nonexistent".to_string()).await.unwrap();
    assert!(result.is_none());

    // Test get_some on empty dataset
    let result = table.get_some().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_insertable_dataset() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    // Test insert with no ID - should generate one
    let user = User {
        id: None,
        name: "Alice".to_string(),
        age: 30,
        active: true,
    };
    let id = table.insert_return_id(&user).await.unwrap();
    assert!(!id.is_empty());

    // Test insert with existing ID - should use it
    let user_with_id = User {
        id: Some("user-123".to_string()),
        name: "Bob".to_string(),
        age: 25,
        active: false,
    };
    let id = table.insert_return_id(&user_with_id).await.unwrap();
    assert_eq!(id, "user-123");

    // Test insert with empty ID - should generate one
    let user_empty_id = User {
        id: Some("".to_string()),
        name: "Charlie".to_string(),
        age: 35,
        active: true,
    };
    let id = table.insert_return_id(&user_empty_id).await.unwrap();
    assert!(!id.is_empty());
    assert_ne!(id, "");
}

#[tokio::test]
async fn test_writable_dataset() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    let user = User {
        id: Some("user-1".to_string()),
        name: "Alice".to_string(),
        age: 30,
        active: true,
    };

    // Test insert (idempotent)
    let result = table.insert("user-1", &user).await.unwrap();
    assert_eq!(result, user);

    // Test insert again - should return existing
    let user2 = User {
        id: Some("user-1".to_string()),
        name: "Different Name".to_string(),
        age: 25,
        active: false,
    };
    let result = table.insert("user-1", &user2).await.unwrap();
    assert_eq!(result, user); // Should return original, not new data

    // Test replace
    let updated_user = User {
        id: Some("user-1".to_string()),
        name: "Alice Updated".to_string(),
        age: 31,
        active: false,
    };
    let result = table.replace("user-1", &updated_user).await.unwrap();
    assert_eq!(result, updated_user);

    // Test patch
    let patch_user = User {
        id: Some("user-1".to_string()),
        name: "Alice Patched".to_string(),
        age: 32,
        active: true,
    };
    let result = table.patch("user-1", &patch_user).await.unwrap();
    assert_eq!(result.name, "Alice Patched");
    assert_eq!(result.age, 32);
    assert!(result.active);

    // Test delete
    table.delete("user-1").await.unwrap();
    let result = table.get("user-1").await.unwrap();
    assert!(result.is_none());

    // Test delete_all
    let _ = table.insert("user-2", &user).await.unwrap();
    let _ = table.insert("user-3", &user).await.unwrap();

    table.delete_all().await.unwrap();
    let result = table.list().await.unwrap();
    assert_eq!(result.len(), 0);
}

#[tokio::test]
async fn test_error_conditions() {
    let ds = ImDataSource::new();
    let table = ImTable::<User>::new(&ds, "users");

    // Test patch on non-existent record
    let user = User {
        id: Some("nonexistent".to_string()),
        name: "Test".to_string(),
        age: 30,
        active: true,
    };
    let result = table.patch("nonexistent", &user).await;
    assert!(result.is_err());

    // Test get on non-existent record
    let result = table.get("nonexistent".to_string()).await.unwrap();
    assert!(result.is_none());
}

// Regression: concurrent writers to the same table must not lose each other's
// rows. Before the lock-held-across-read-modify-write fix, each writer cloned a
// snapshot, mutated it, and wrote it back — so racing inserts of distinct ids
// would clobber one another and only the last writer's row survived.
#[tokio::test]
async fn test_concurrent_inserts_no_lost_update() {
    let ds = ImDataSource::new();

    const N: usize = 200;
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let table = ImTable::<User>::new(&ds, "users");
        handles.push(tokio::spawn(async move {
            let user = User {
                id: Some(format!("user{i}")),
                name: format!("u{i}"),
                age: i as i32,
                active: true,
            };
            table.insert(format!("user{i}"), &user).await.unwrap();
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    let table = ImTable::<User>::new(&ds, "users");
    let all = table.list().await.unwrap();
    assert_eq!(all.len(), N, "every concurrent insert must survive");
}

#[tokio::test]
async fn insert_surfaces_serialization_error_instead_of_panicking() {
    let ds = ImDataSource::new();
    let table = ImTable::<Unserializable>::new(&ds, "boom");

    let result = table.insert("k", &Unserializable).await;

    assert!(
        result.is_err(),
        "a serialization failure must be a recoverable error, not a panic"
    );
}
