//! The write contract each SQL backend runs: confined, idempotent,
//! invariant-filling.
//!
//! A backend's `safe_writes` module imports its `Table`, `EmptyEntity` and
//! dialect operation trait, defines `async fn item(test: &str)` — an empty
//! `item` table with a text `id` and `name`, `parent`, `price` columns, kept
//! apart from other tests by `test` where the database is shared — and
//! invokes `safe_writes_tests!(DB, ValueType)`.

macro_rules! safe_writes_tests {
    ($db:ty, $value:ty) => {
        use vantage_dataset::contract::{self, Fixture, OpFixture};
        use vantage_dataset::prelude::*;

        type Item = Table<$db, EmptyEntity>;

        fn rec(pairs: &[(&str, $value)]) -> vantage_types::Record<$value> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect()
        }

        /// `in1` (parent p1) and `out1` (parent p2), and the set narrowed to
        /// `parent = "p1"`.
        async fn fixture(test: &str) -> (Item, Item) {
            let all = item(test).await;
            all.insert_value(
                "in1",
                &rec(&[("name", "a".into()), ("parent", "p1".into())]),
            )
            .await
            .unwrap();
            all.insert_value(
                "out1",
                &rec(&[("name", "b".into()), ("parent", "p2".into())]),
            )
            .await
            .unwrap();
            let parent = all["parent"].clone();
            let set = all.clone().with_condition(parent.eq("p1"));
            (all, set)
        }

        confined_check!(delete_is_confined, check_delete, $value);
        confined_check!(insert_is_confined, check_insert, $value);
        confined_check!(patch_is_confined, check_patch, $value);
        confined_check!(replace_is_confined, check_replace, $value);
        confined_check!(delete_all_is_confined, check_delete_all, $value);

        #[tokio::test]
        async fn where_eq_fills_on_insert() {
            let (all, set) = fixture("where_eq_fills_on_insert").await;
            set.insert_value("n9", &rec(&[("name", "z".into())]))
                .await
                .unwrap();
            assert_eq!(
                all.get_value("n9").await.unwrap().unwrap()["parent"],
                <$value>::from("p1".to_string())
            );
        }

        #[tokio::test]
        async fn operator_condition_keeps_rows_in_set() {
            let all = item("operator_condition_keeps_rows_in_set").await;
            all.insert_value(
                "cheap",
                &rec(&[("name", "c".into()), ("price", 5i64.into())]),
            )
            .await
            .unwrap();
            all.insert_value(
                "dear",
                &rec(&[("name", "d".into()), ("price", 20i64.into())]),
            )
            .await
            .unwrap();
            let price = all["price"].clone();
            let set = all.clone().with_condition(price.gt(10i64));
            contract::check_operator_condition(&OpFixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text: |s| <$value>::from(s.to_string()),
                int: |n| <$value>::from(n),
            })
            .await;
        }
    };
}

/// One `vantage_dataset::contract` check over [`safe_writes_tests!`]'s
/// fixture, as a test named `$name`.
macro_rules! confined_check {
    ($name:ident, $check:ident, $value:ty) => {
        #[tokio::test]
        async fn $name() {
            let (all, set) = fixture(stringify!($name)).await;
            contract::$check(&Fixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text: |s| <$value>::from(s.to_string()),
                detects_outside: true,
            })
            .await;
        }
    };
}
