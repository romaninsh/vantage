//! Test 4: Table definition over Redb — columns, flags, id_field.

use vantage_redb::Redb;
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::EmptyEntity;

fn fresh_db() -> (tempfile::NamedTempFile, Redb) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    (path, db)
}

#[test]
fn test_table_collects_columns_in_order() {
    let (_tmp, db) = fresh_db();
    let table = Table::<Redb, EmptyEntity>::new("product", db)
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<bool>("is_deleted");

    let names: Vec<&String> = table.columns().keys().collect();
    assert_eq!(names, vec!["name", "price", "is_deleted"]);
}

#[test]
fn test_table_with_id_column_resolves_id_field() {
    let (_tmp, db) = fresh_db();
    let table = Table::<Redb, EmptyEntity>::new("notes", db)
        .with_id_column("id")
        .with_column_of::<String>("body");

    let id = table.id_field().expect("id_field set");
    assert_eq!(ColumnLike::name(id), "id");
}

#[test]
fn test_table_without_id_column_has_none_id_field() {
    let (_tmp, db) = fresh_db();
    let table = Table::<Redb, EmptyEntity>::new("anon", db).with_column_of::<String>("body");

    assert!(table.id_field().is_none());
}

#[test]
fn test_indexed_flag_visible_on_column() {
    let indexed_email = Column::<String>::new("email").with_flag(ColumnFlag::Indexed);

    let (_tmp, db) = fresh_db();
    let table = Table::<Redb, EmptyEntity>::new("users", db)
        .with_column_of::<String>("name")
        .with_column(indexed_email);

    let email = &table["email"];
    assert!(email.flags().contains(&ColumnFlag::Indexed));

    let name = &table["name"];
    assert!(!name.flags().contains(&ColumnFlag::Indexed));
}

#[test]
fn test_table_carries_data_source() {
    let (_tmp, db) = fresh_db();
    let table = Table::<Redb, EmptyEntity>::new("things", db.clone());
    // We can re-use the same Redb handle; cloning is cheap (Arc).
    let _: &Redb = table.data_source();
}
