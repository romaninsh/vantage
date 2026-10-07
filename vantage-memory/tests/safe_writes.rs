//! The write contract on memory: typed Table and Vista shell.

use ciborium::Value as CborValue;
use vantage_dataset::contract::{self, Fixture, OpFixture};
use vantage_dataset::prelude::*;
use vantage_memory::typed::operation::MemoryOperation;
use vantage_memory::vista::Catalog;
use vantage_memory::{AnyMemoryType, MemoryDB, MemoryStore, MemoryTableShell};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Column, FilterOp, Vista, VistaMetadata};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn seeded() -> MemoryStore {
    let store = MemoryStore::new();
    let t = store.table("item");
    let row = |n: &str, p: &str| -> Record<CborValue> {
        [
            ("name".to_string(), text(n)),
            ("parent".to_string(), text(p)),
        ]
        .into_iter()
        .collect()
    };
    t.insert_as("in1", row("a", "p1")).unwrap();
    t.insert_as("out1", row("b", "p2")).unwrap();
    store
}

fn vista(store: &MemoryStore) -> Vista {
    let md = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Vista::new(
        "item",
        Box::new(MemoryTableShell::new(
            store.table("item"),
            md,
            Catalog::new(store.clone()),
        )),
    )
}

macro_rules! vista_check {
    ($name:ident, $check:path) => {
        #[tokio::test]
        async fn $name() {
            let store = seeded();
            let all = vista(&store);
            let mut set = vista(&store);
            set.add_condition_eq("parent", text("p1")).unwrap();
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

vista_check!(vista_delete_is_confined, contract::check_delete);
vista_check!(vista_insert_is_confined, contract::check_insert);
vista_check!(vista_patch_is_confined, contract::check_patch);
vista_check!(vista_replace_is_confined, contract::check_replace);
vista_check!(vista_delete_all_is_confined, contract::check_delete_all);

macro_rules! typed_check {
    ($name:ident, $check:path) => {
        #[tokio::test]
        async fn $name() {
            let db = MemoryDB::from_store(seeded());
            let all = Table::<MemoryDB, EmptyEntity>::new("item", db)
                .with_id_column("id")
                .with_column_of::<String>("name")
                .with_column_of::<String>("parent");
            let set = all.clone().with_condition(all["parent"].eq("p1"));
            $check(&Fixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text: |s| AnyMemoryType::from(s),
                detects_outside: true,
            })
            .await;
        }
    };
}

typed_check!(typed_delete_is_confined, contract::check_delete);
typed_check!(typed_insert_is_confined, contract::check_insert);
typed_check!(typed_patch_is_confined, contract::check_patch);
typed_check!(typed_replace_is_confined, contract::check_replace);

#[tokio::test]
async fn vista_operator_condition_keeps_rows_in_set() {
    let store = MemoryStore::new();
    let t = store.table("item");
    let row = |n: i64| -> Record<CborValue> {
        [("price".to_string(), CborValue::Integer(n.into()))]
            .into_iter()
            .collect()
    };
    t.insert_as("cheap", row(5)).unwrap();
    t.insert_as("dear", row(20)).unwrap();
    let all = vista(&store);
    let mut set = vista(&store);
    set.add_condition("price", FilterOp::Gt, CborValue::Integer(10.into()))
        .unwrap();
    contract::check_operator_condition(&OpFixture {
        all: &all,
        set: &set,
        id: |s| s.to_string(),
        text,
        int: |n| CborValue::Integer(n.into()),
    })
    .await;
}

#[tokio::test]
async fn native_import_never_overwrites() {
    let store = seeded();
    let all = vista(&store);
    let rows: indexmap::IndexMap<String, Record<CborValue>> = [
        (
            "in1".to_string(),
            [("name".to_string(), text("z"))].into_iter().collect(),
        ),
        (
            "new".to_string(),
            [("name".to_string(), text("n"))].into_iter().collect(),
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(all.import_values(&rows).await.unwrap(), 1);
    assert_eq!(
        all.get_value("in1").await.unwrap().unwrap()["name"],
        text("a"),
        "import must not overwrite"
    );
}
