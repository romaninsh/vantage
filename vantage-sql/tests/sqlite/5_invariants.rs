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

/// A set traversed from a parent carries `parent_id = <parent>` as an
/// invariant: an absent FK is filled, a matching one kept, a conflicting one
/// rejected on insert and on patch. The untyped ActiveRecord handle scopes the
/// same way.
#[tokio::test]
async fn scoped_set_enforces_its_foreign_key() {
    let db = setup().await;
    let parents = parent_table(db.clone());
    let children_table = child_table(db.clone());

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
    let children = parent.get_ref::<Child>("children").unwrap();

    let filled = children
        .insert_return_id(&Child {
            name: "Eagle".into(),
            parent_id: None,
        })
        .await
        .unwrap();
    let filled_child = children_table
        .get(filled.clone())
        .await
        .unwrap()
        .expect("filled");
    assert_eq!(filled_child.parent_id.as_deref(), Some(parent_id.as_str()));

    let kept = children
        .insert_return_id(&Child {
            name: "matches".into(),
            parent_id: Some(parent_id.clone()),
        })
        .await
        .unwrap();
    let kept_child = children_table.get(kept).await.unwrap().expect("kept");
    assert_eq!(kept_child.parent_id.as_deref(), Some(parent_id.as_str()));

    let conflicting = Child {
        name: "wrong".into(),
        parent_id: Some("someone-else".into()),
    };
    assert!(children.insert_return_id(&conflicting).await.is_err());
    assert!(children.patch(filled, &conflicting).await.is_err());

    let via_record = parents
        .get_value_record(parent_id.clone())
        .await
        .unwrap()
        .expect("parent record loaded")
        .get_ref::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "Titan".into(),
            parent_id: None,
        })
        .await
        .unwrap();
    let record_child = children_table
        .get(via_record)
        .await
        .unwrap()
        .expect("via record");
    assert_eq!(record_child.parent_id.as_deref(), Some(parent_id.as_str()));
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

    let a_children = parents
        .get_entity(a)
        .await
        .unwrap()
        .unwrap()
        .get_ref::<Child>("children")
        .unwrap();
    let a1 = a_children
        .insert_return_id(&Child {
            name: "a1".into(),
            parent_id: None,
        })
        .await
        .unwrap();
    parents
        .get_entity(b)
        .await
        .unwrap()
        .unwrap()
        .get_ref::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "b1".into(),
            parent_id: None,
        })
        .await
        .unwrap();

    let listed = a_children.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed.contains_key(&a1));
}
