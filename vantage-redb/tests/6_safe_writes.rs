//! The write contract on redb: every write is retry-safe and confined to the
//! table's narrowed set.

use vantage_dataset::contract::{self, Fixture};
use vantage_dataset::prelude::*;
use vantage_redb::{AnyRedbType, Redb, RedbCondition};
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

fn fresh_table(name: &str) -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let table = Table::<Redb, EmptyEntity>::new(name, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column(Column::<String>::new("parent").with_flag(ColumnFlag::Indexed));
    (path, table)
}

fn record(fields: &[(&str, AnyRedbType)]) -> Record<AnyRedbType> {
    fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn text(s: &str) -> AnyRedbType {
    AnyRedbType::new(s.to_string())
}

async fn seeded() -> (tempfile::NamedTempFile, Table<Redb, EmptyEntity>) {
    let (f, all) = fresh_table("item");
    all.insert_value(
        "in1",
        &record(&[("name", text("a")), ("parent", text("p1"))]),
    )
    .await
    .unwrap();
    all.insert_value(
        "out1",
        &record(&[("name", text("b")), ("parent", text("p2"))]),
    )
    .await
    .unwrap();
    (f, all)
}

macro_rules! check {
    ($name:ident, $check:path) => {
        #[tokio::test]
        async fn $name() {
            let (_f, all) = seeded().await;
            let set = all
                .clone()
                .with_condition(RedbCondition::eq("parent", "p1"));
            $check(&Fixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text,
                detects_outside: true,
            })
            .await;
        }
    };
}

check!(delete_is_confined, contract::check_delete);
check!(insert_is_confined, contract::check_insert);
check!(patch_is_confined, contract::check_patch);
check!(replace_is_confined, contract::check_replace);
check!(delete_all_is_confined, contract::check_delete_all);

#[tokio::test]
async fn unnarrowed_table_reads_and_writes_everything() {
    let (_f, all) = seeded().await;

    contract::check_unnarrowed(&all, |s| s.to_string(), text).await;
}

#[tokio::test]
async fn insert_never_leaves_stale_index_entries() {
    let (_f, t) = fresh_table("item");
    t.insert_value("x", &record(&[("parent", "p1".into())]))
        .await
        .unwrap();
    t.insert_value("x", &record(&[("parent", "p2".into())]))
        .await
        .unwrap();
    let p2 = t.clone().with_condition(RedbCondition::eq("parent", "p2"));
    assert!(
        p2.list_values().await.unwrap().is_empty(),
        "the second insert must not index p2"
    );
}
