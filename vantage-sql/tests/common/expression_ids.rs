//! A table whose id is an expression over its columns, as a link table's is:
//! inserts get the id from the database, a duplicate settles to `Existed`
//! by the record, and by-id reads and writes compare the expression.
//!
//! A backend's `expression_ids` module defines `async fn link(test: &str)`:
//! an empty `link` table with integer `a`, `b` and `stage` columns,
//! `UNIQUE (a, b)` and `b` nullable, whose `id` is the expression `a:b`, kept
//! apart from other tests by `test` where the database is shared, and
//! `strict_link(test)`, the same with `b NOT NULL`. It then invokes
//! `expression_ids_tests!(DB, ValueType)`.

macro_rules! expression_ids_tests {
    ($db:ty, $value:ty) => {
        use vantage_dataset::Inserted;
        use vantage_dataset::prelude::*;

        type Link = Table<$db, EmptyEntity>;

        fn rec(pairs: &[(&str, i64)]) -> vantage_types::Record<$value> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), <$value>::from(*v)))
                .collect()
        }

        fn id_of(row: &vantage_types::Record<$value>) -> $value {
            row.get("id").cloned().expect("row carries its id")
        }

        /// The link set of parent `a`: `a = <a>` is its invariant.
        fn of(all: &Link, a: i64) -> Link {
            let column = all["a"].clone();
            all.clone().with_condition(column.eq(a))
        }

        #[tokio::test]
        async fn expression_id_insert_returns_db_computed_id() {
            let all = link("expression_id_insert_returns_db_computed_id").await;
            let id = of(&all, 7)
                .insert_return_id_value(&rec(&[("b", 5)]))
                .await
                .unwrap();
            assert_eq!(id, "7:5");
            let row = all.get_value("7:5").await.unwrap().expect("row 7:5");
            assert_eq!(id_of(&row), <$value>::from("7:5".to_string()));
        }

        #[tokio::test]
        async fn expression_id_duplicate_insert_is_existed() {
            let all = link("expression_id_duplicate_insert_is_existed").await;
            let set = of(&all, 7);
            let (id, first) = set.insert_derived_value(&rec(&[("b", 5)])).await.unwrap();
            assert_eq!(id, "7:5");
            let Inserted::Created(created) = first else {
                panic!("a new pair is Created, got: {first:?}");
            };
            assert_eq!(id_of(&created), <$value>::from("7:5".to_string()));

            let (id, second) = set.insert_derived_value(&rec(&[("b", 5)])).await.unwrap();
            assert_eq!(id, "7:5");
            let Inserted::Existed(existed) = second else {
                panic!("a stored pair is Existed, got: {second:?}");
            };
            assert_eq!(id_of(&existed), <$value>::from("7:5".to_string()));
            assert_eq!(all.get_count().await.unwrap(), 1);
        }

        #[tokio::test]
        async fn expression_id_get_patch_delete_by_id() {
            let all = link("expression_id_get_patch_delete_by_id").await;
            let set = of(&all, 7);
            set.insert_return_id_value(&rec(&[("b", 5)])).await.unwrap();
            assert!(set.get_value("7:5").await.unwrap().is_some());

            let patched = set.patch_value("7:5", &rec(&[("stage", 4)])).await.unwrap();
            assert_eq!(patched.get("stage"), Some(&<$value>::from(4i64)));

            of(&all, 8).delete("7:5").await.unwrap();
            assert_eq!(all.get_count().await.unwrap(), 1, "outside the set");
            set.delete("7:5").await.unwrap();
            assert_eq!(all.get_count().await.unwrap(), 0);
        }

        #[tokio::test]
        async fn expression_id_null_part_is_an_error() {
            let all = link("expression_id_null_part_is_an_error").await;
            let err = of(&all, 7)
                .insert_return_id_value(&rec(&[]))
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("returned a NULL id"), "{err}");
            assert!(err.contains("link"), "names the table: {err}");
        }

        /// A failed insert is the failure, never another row's `Existed`:
        /// a record leaving out a NOT NULL column fails, though the set holds
        /// a row matching what the record does set.
        #[tokio::test]
        async fn expression_id_failed_insert_is_not_existed() {
            let all = strict_link("expression_id_failed_insert_is_not_existed").await;
            let set = of(&all, 7);
            set.insert_derived_value(&rec(&[("b", 5)])).await.unwrap();
            let out = set.insert_derived_value(&rec(&[])).await;
            assert!(out.is_err(), "a NOT NULL failure, got: {out:?}");
            assert_eq!(all.get_count().await.unwrap(), 1);
        }

        /// A record with a NULL column is matched by `IS NULL`: linking it
        /// again reports the stored row.
        #[tokio::test]
        async fn expression_id_null_column_relink_is_existed() {
            let all = link("expression_id_null_column_relink_is_existed").await;
            let set = of(&all, 7);
            let mut link = rec(&[("b", 5)]);
            link.insert("stage".into(), <$value>::from(ciborium::Value::Null));
            let (_, first) = set.insert_derived_value(&link).await.unwrap();
            assert!(matches!(first, Inserted::Created(_)), "{first:?}");
            let (id, second) = set.insert_derived_value(&link).await.unwrap();
            assert_eq!(id, "7:5");
            assert!(matches!(second, Inserted::Existed(_)), "{second:?}");
            assert_eq!(all.get_count().await.unwrap(), 1);
        }

        #[tokio::test]
        async fn expression_id_insert_with_an_id_is_refused() {
            let all = link("expression_id_insert_with_an_id_is_refused").await;
            let err = of(&all, 7)
                .insert_value("7:5", &rec(&[("b", 5)]))
                .await
                .unwrap_err();
            assert!(err.is_unsupported(), "{err}");
            assert_eq!(all.get_count().await.unwrap(), 0);
        }
    };
}
