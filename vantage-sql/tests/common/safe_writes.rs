//! The write contract each SQL backend runs: confined, idempotent,
//! invariant-filling.
//!
//! A backend's `safe_writes` module imports its `Table`, `EmptyEntity` and
//! dialect operation trait, defines `async fn item(test: &str)` — an empty
//! `item` table with a text `id`, `name`, `parent`, `price` columns and a
//! UNIQUE `email`, kept
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
        async fn list_condition_keeps_writes_in_the_list() {
            use vantage_table::traits::table_source::TableSource;
            let (all, _) = fixture("list_condition_keeps_writes_in_the_list").await;
            let listed = all
                .data_source()
                .in_values_condition("parent", vec!["p1".into(), "p3".into()])
                .unwrap();
            let set = all.clone().with_condition(listed);
            contract::check_list_condition(&Fixture {
                all: &all,
                set: &set,
                id: |s| s.to_string(),
                text: |s| <$value>::from(s.to_string()),
                detects_outside: true,
            })
            .await;
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

        /// A second insert of a stored id writes nothing and reports the
        /// stored row as `Existed`.
        #[tokio::test]
        async fn sql_insert_existing_id_reports_existed() {
            use vantage_dataset::Inserted;
            let all = item("insert_existing_id_reports_existed").await;
            let name = |s: &str| <$value>::from(s.to_string());
            let first = all
                .insert_value("a", &rec(&[("name", "first".into())]))
                .await
                .unwrap();
            let Inserted::Created(created) = first else {
                panic!("a new id is Created, got: {first:?}");
            };
            assert_eq!(created.get("name"), Some(&name("first")));

            let second = all
                .insert_value("a", &rec(&[("name", "second".into())]))
                .await
                .unwrap();
            let Inserted::Existed(existed) = second else {
                panic!("a stored id is Existed, got: {second:?}");
            };
            assert_eq!(existed.get("name"), Some(&name("first")));
            let stored = all.get_value("a").await.unwrap().expect("row a");
            assert_eq!(stored.get("name"), Some(&name("first")));
        }

        #[tokio::test]
        async fn operator_condition_on_absent_column_is_a_conflict() {
            let all = item("operator_condition_on_absent_column_is_a_conflict").await;
            let price = all["price"].clone();
            let set = all.clone().with_condition(price.gt(10i64));
            let err = set
                .insert_value("nop", &rec(&[("name", "n".into())]))
                .await
                .expect_err("a row without a price is outside `price > 10`");
            assert!(err.is_conflict(), "expected a conflict, got: {err:?}");
            assert!(all.get_value("nop").await.unwrap().is_none());
        }

        type Seen = std::sync::Arc<std::sync::Mutex<Vec<(Option<$value>, Option<$value>)>>>;

        /// An after-update hook recording `(before.name, record.name)`.
        fn name_before_and_after(seen: Seen) -> vantage_table::prelude::Hook<$db> {
            vantage_table::prelude::Hook::AfterUpdate(vantage_table::table::after_hook(
                move |cx: vantage_table::table::AfterCx<'_, $db>| {
                    let pair = (
                        cx.before.and_then(|b| b.get("name").cloned()),
                        cx.record.get("name").cloned(),
                    );
                    let seen = seen.clone();
                    Box::pin(async move {
                        seen.lock().unwrap().push(pair);
                        Ok(())
                    })
                },
            ))
        }

        /// `in1` is named `a`; the update names it `z`.
        async fn assert_after_update_sees_the_former_row(test: &str, patch: bool) {
            let seen = Seen::default();
            let (all, _) = fixture(test).await;
            let hooked = all.with_hook(name_before_and_after(seen.clone()));
            if patch {
                hooked
                    .patch_value("in1", &rec(&[("name", "z".into())]))
                    .await
                    .unwrap();
            } else {
                hooked
                    .replace_value(
                        "in1",
                        &rec(&[("name", "z".into()), ("parent", "p1".into())]),
                    )
                    .await
                    .unwrap();
            }
            let text = |s: &str| Some(<$value>::from(s.to_string()));
            assert_eq!(*seen.lock().unwrap(), vec![(text("a"), text("z"))]);
        }

        #[tokio::test]
        async fn after_update_sees_the_former_row_on_replace() {
            assert_after_update_sees_the_former_row("after_update_former_replace", false).await;
        }

        #[tokio::test]
        async fn after_update_sees_the_former_row_on_patch() {
            assert_after_update_sees_the_former_row("after_update_former_patch", true).await;
        }

        /// Only an id clash may turn a replace into an update: a clash on
        /// another UNIQUE column is an error and leaves that row alone,
        /// whether the replaced id exists (`a`) or not (`c`).
        #[tokio::test]
        async fn replace_does_not_touch_a_row_holding_the_same_unique_key() {
            let all = item("replace_unique_key").await;
            let email = |s: &str| <$value>::from(s.to_string());
            for (id, mail) in [("a", "a@x.test"), ("b", "b@x.test")] {
                all.insert_value(id, &rec(&[("name", id.into()), ("email", email(mail))]))
                    .await
                    .unwrap();
            }
            for id in ["a", "c"] {
                all.replace_value(
                    id,
                    &rec(&[("name", "z".into()), ("email", email("b@x.test"))]),
                )
                .await
                .expect_err("b@x.test belongs to row b");
                let b = all.get_value("b").await.unwrap().expect("row b survives");
                assert_eq!(b.get("name"), Some(&<$value>::from("b".to_string())));
                assert_eq!(b.get("email"), Some(&email("b@x.test")));
            }
            let a = all.get_value("a").await.unwrap().expect("row a survives");
            assert_eq!(a.get("name"), Some(&<$value>::from("a".to_string())));
            assert!(all.get_value("c").await.unwrap().is_none());
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
