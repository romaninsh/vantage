//! Test 3: the `Redb` handle: create, open, wrap, clone, and persistence.

use vantage_dataset::prelude::*;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

fn notes(db: Redb) -> Table<Redb, EmptyEntity> {
    Table::<Redb, EmptyEntity>::new("notes", db)
        .with_id_column("id")
        .with_column_of::<String>("body")
}

fn note(body: &str) -> Record<AnyRedbType> {
    let mut r: Record<AnyRedbType> = Record::new();
    r.insert("body".into(), AnyRedbType::new(body.to_string()));
    r
}

#[test]
fn test_open_of_a_missing_file_errors() {
    let dir = tempfile::tempdir().unwrap();

    assert!(Redb::open(dir.path().join("absent.redb")).is_err());
}

#[test]
fn test_create_makes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.redb");

    Redb::create(&path).unwrap();

    assert!(path.exists());
}

#[tokio::test]
async fn test_rows_survive_reopening_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keep.redb");
    {
        let table = notes(Redb::create(&path).unwrap());
        table.insert_value("n1", &note("hello")).await.unwrap();
    }

    let reopened = notes(Redb::open(&path).unwrap());

    let row = reopened
        .get_value("n1")
        .await
        .unwrap()
        .expect("n1 persisted");
    assert_eq!(row["body"].try_get::<String>(), Some("hello".into()));
}

#[tokio::test]
async fn test_create_on_an_existing_file_opens_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("again.redb");
    {
        let table = notes(Redb::create(&path).unwrap());
        table.insert_value("n1", &note("hello")).await.unwrap();
    }

    let again = notes(Redb::create(&path).unwrap());

    assert_eq!(again.list_values().await.unwrap().len(), 1);
}

#[tokio::test]
async fn test_clones_share_the_same_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = Redb::create(dir.path().join("shared.redb")).unwrap();
    let writer = notes(db.clone());
    let reader = notes(db);

    writer.insert_value("n1", &note("hello")).await.unwrap();

    assert_eq!(reader.list_values().await.unwrap().len(), 1);
}

#[tokio::test]
async fn test_from_database_wraps_a_redb_database() {
    let dir = tempfile::tempdir().unwrap();
    let inner = redb::Database::create(dir.path().join("wrapped.redb")).unwrap();
    let table = notes(Redb::from_database(inner));

    table.insert_value("n1", &note("hello")).await.unwrap();

    assert!(table.get_value("n1").await.unwrap().is_some());
}

#[tokio::test]
async fn test_database_borrows_the_wrapped_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = Redb::create(dir.path().join("borrow.redb")).unwrap();

    assert!(db.database().begin_read().is_ok());
}

#[tokio::test]
async fn test_tables_with_different_names_do_not_see_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let db = Redb::create(dir.path().join("two.redb")).unwrap();
    let notes = notes(db.clone());
    let other = Table::<Redb, EmptyEntity>::new("other", db)
        .with_id_column("id")
        .with_column_of::<String>("body");

    notes.insert_value("n1", &note("hello")).await.unwrap();

    assert!(other.list_values().await.unwrap().is_empty());
}
