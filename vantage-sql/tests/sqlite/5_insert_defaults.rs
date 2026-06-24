//! Test 5: insert-time defaults from an equality scope, and `ActiveEntity::related`.
//!
//! A table narrowed by a literal `column = value` (via `with_id` or relationship
//! traversal) carries that value as an insert default, so a new row conforms to
//! the set. Covers Gap 1 (`related` on a loaded record) + Gap 2 (FK auto-fill).

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::prelude::{ActiveEntitySet, InsertableDataSet, ReadableDataSet};
use vantage_table::prelude::RelatedEntityExt;

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
async fn related_insert_fills_foreign_key() {
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
        .related::<Child>("children")
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

/// A caller-supplied non-null value is never overwritten by the default.
#[tokio::test]
async fn caller_value_wins_over_default() {
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
        .related::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "explicit".into(),
            parent_id: Some("someone-else".into()),
        })
        .await
        .unwrap();

    let child = child_table(db.clone())
        .get(child_id)
        .await
        .unwrap()
        .expect("child loaded");
    assert_eq!(child.parent_id.as_deref(), Some("someone-else"));
}

/// `related` returns a set scoped to the parent: only that parent's children.
#[tokio::test]
async fn related_returns_scoped_set() {
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
            .related::<Child>("children")
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
        .related::<Child>("children")
        .unwrap()
        .insert_return_id(&Child {
            name: "b1".into(),
            parent_id: None,
        })
        .await
        .unwrap();

    let a_children = parent_a.related::<Child>("children").unwrap();
    assert_eq!(a_children.list().await.unwrap().len(), 2);
}
