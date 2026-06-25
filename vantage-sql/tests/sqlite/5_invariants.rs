//! Test 5: set invariants from an equality scope, and `ActiveEntity::get_ref`.
//!
//! A table narrowed by a literal `column = value` (via `with_id` or relationship
//! traversal) carries that value as an invariant, so every row written into the
//! set conforms: an absent/null column is filled, a matching value is kept, and
//! a conflicting value is rejected. Covers `get_ref` traversal + invariant
//! enforcement on insert and patch.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ActiveRecordSet;
use vantage_dataset::prelude::{
    ActiveEntitySet, InsertableDataSet, ReadableDataSet, WritableDataSet,
};
use vantage_table::prelude::GetRefExt;

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Parent {
    name: String,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Child {
    name: String,
    parent_id: Option<String>,
}

fn parent_table(db: SqliteDB) -> Table<SqliteDB, Parent> {
    Table::new("parent", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("children", "parent_id", child_table)
}

fn child_table(db: SqliteDB) -> Table<SqliteDB, Child> {
    Table::new("child", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<Option<String>>("parent_id")
        .with_one("parent", "parent_id", parent_table)
}

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    // id columns auto-generate so `insert_return_id` works without an explicit id.
    sqlx::query(
        "CREATE TABLE parent (
            id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
            name TEXT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();
    sqlx::query(
        "CREATE TABLE child (
            id TEXT PRIMARY KEY DEFAULT (lower(hex(randomblob(16)))),
            name TEXT NOT NULL,
            parent_id TEXT
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();
    db
}

/// Gap 1 + Gap 2: traverse from a loaded parent and insert a child with no FK —
/// the foreign key is filled from the set's definition.
#[tokio::test]
async fn get_ref_insert_fills_foreign_key() {
    let db = setup().await;
    let parents = parent_table(db.clone());

    let parent_id = parents
        .insert_return_id(&Parent {
            name: "Apollo".into(),
        })
        .await
        .unwrap();

    let parent = parents
        .get_entity(parent_id.clone())
        .await
        .unwrap()
        .expect("parent loaded");

    // Child entered without parent_id — the relation supplies it.
    let child_id = parent
        .get_ref::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "Eagle".into(),
            parent_id: None,
        })
        .await
        .unwrap();

    let child = child_table(db.clone())
        .get(child_id)
        .await
        .unwrap()
        .expect("child loaded");
    assert_eq!(child.parent_id.as_deref(), Some(parent_id.as_str()));
}

/// A caller-supplied value that matches the set's invariant is kept (no error).
#[tokio::test]
async fn caller_matching_value_is_kept() {
    let db = setup().await;
    let parents = parent_table(db.clone());

    let parent_id = parents
        .insert_return_id(&Parent { name: "P".into() })
        .await
        .unwrap();
    let parent = parents
        .get_entity(parent_id.clone())
        .await
        .unwrap()
        .expect("parent loaded");

    let child_id = parent
        .get_ref::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "matches".into(),
            parent_id: Some(parent_id.clone()), // same as the scope
        })
        .await
        .unwrap();

    let child = child_table(db.clone())
        .get(child_id)
        .await
        .unwrap()
        .expect("child loaded");
    assert_eq!(child.parent_id.as_deref(), Some(parent_id.as_str()));
}

/// A caller-supplied value that conflicts with the set's invariant is rejected:
/// the row does not belong to this set.
#[tokio::test]
async fn caller_conflict_errors() {
    let db = setup().await;
    let parents = parent_table(db.clone());

    let parent_id = parents
        .insert_return_id(&Parent { name: "P".into() })
        .await
        .unwrap();
    let parent = parents
        .get_entity(parent_id.clone())
        .await
        .unwrap()
        .expect("parent loaded");

    let result = parent
        .get_ref::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "wrong".into(),
            parent_id: Some("someone-else".into()),
        })
        .await;

    assert!(result.is_err(), "conflicting FK must be rejected");
}

/// Invariants are enforced on patch too: a conflicting foreign key is rejected.
#[tokio::test]
async fn patch_conflict_errors() {
    let db = setup().await;
    let parents = parent_table(db.clone());

    let parent_id = parents
        .insert_return_id(&Parent { name: "P".into() })
        .await
        .unwrap();
    let parent = parents
        .get_entity(parent_id.clone())
        .await
        .unwrap()
        .expect("parent loaded");
    let children = parent.get_ref::<Child>("children").unwrap();

    let child_id = children
        .insert_return_id(&Child {
            name: "ok".into(),
            parent_id: None,
        })
        .await
        .unwrap();

    // Patching the same scoped set with a different parent_id is a conflict.
    let result = children
        .patch(
            child_id,
            &Child {
                name: "moved".into(),
                parent_id: Some("someone-else".into()),
            },
        )
        .await;

    assert!(
        result.is_err(),
        "patch with conflicting FK must be rejected"
    );
}

/// `get_ref` returns a set scoped to the parent: only that parent's children.
#[tokio::test]
async fn get_ref_returns_scoped_set() {
    let db = setup().await;
    let parents = parent_table(db.clone());

    let a = parents
        .insert_return_id(&Parent { name: "A".into() })
        .await
        .unwrap();
    let b = parents
        .insert_return_id(&Parent { name: "B".into() })
        .await
        .unwrap();

    let parent_a = parents.get_entity(a.clone()).await.unwrap().unwrap();
    for n in ["a1", "a2"] {
        parent_a
            .get_ref::<Child>("children")
            .unwrap()
            .insert_return_id(&Child {
                name: n.into(),
                parent_id: None,
            })
            .await
            .unwrap();
    }
    let parent_b = parents.get_entity(b).await.unwrap().unwrap();
    parent_b
        .get_ref::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "b1".into(),
            parent_id: None,
        })
        .await
        .unwrap();

    let a_children = parent_a.get_ref::<Child>("children").unwrap();
    assert_eq!(a_children.list().await.unwrap().len(), 2);
}

/// `get_ref` is available on the untyped `ActiveRecord` handle too: load the
/// parent via `get_value_record` and traverse + insert a child with no FK.
#[tokio::test]
async fn get_ref_on_active_record_fills_foreign_key() {
    let db = setup().await;
    let parents = parent_table(db.clone());

    let parent_id = parents
        .insert_return_id(&Parent {
            name: "Gemini".into(),
        })
        .await
        .unwrap();

    let parent_record = parents
        .get_value_record(parent_id.clone())
        .await
        .unwrap()
        .expect("parent record loaded");

    let child_id = parent_record
        .get_ref::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "Titan".into(),
            parent_id: None,
        })
        .await
        .unwrap();

    let child = child_table(db.clone())
        .get(child_id)
        .await
        .unwrap()
        .expect("child loaded");
    assert_eq!(child.parent_id.as_deref(), Some(parent_id.as_str()));
}
