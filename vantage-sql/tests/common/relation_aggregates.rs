//! Relation expressions each SQL backend renders: an aggregate over the rows
//! a relation gives for each row, narrowed and ordered.
//!
//! The invoking module defines what a dialect differs in: `async fn
//! connect()`, `fn link_id()` (the `a || ':' || b` id of `rx_project_tag`),
//! `TEXT`, its text column type, and `fn insert_ignore(table, values)`. The
//! `rx_*` tables below are shared by every test and filled once; a statement
//! that loses a race with another test's setup is retried.
//!
//! - `rx_project(id, name)`: 7 and 8;
//! - `rx_tag(id, name)`: 3 "alpha", 5 "beta";
//! - `rx_project_tag(a, b)`: project 7 has tags 5 and 3, project 8 none;
//! - `rx_task(id, project_id, parent_id, active)`: 1 (7, -, 1), 2 (7, 1, 1),
//!   3 (7, 1, 0), 4 (8, -, 1), 5 (7, 2, 1);
//! - `rx_update(id, project_id, date, status)`: project 7 "off_track" on
//!   2026-01-01, "on_track" on 2026-02-01.

macro_rules! relation_aggregates_tests {
    ($db:ty, $value:ty) => {
      mod relation_aggregates {
        use super::*;
        use serde_json::{Value as JsonValue, json};
        use vantage_dataset::prelude::*;
        use vantage_table::sorting::SortDirection;
        use vantage_table::table::{Aggregate, RelationQuery};
        use vantage_table::traits::table_source::TableSource;

        type T = Table<$db, EmptyEntity>;

        async fn setup() -> $db {
            let db = connect().await;
            let tables = [
                "rx_project (id INTEGER PRIMARY KEY, name TEXT)",
                "rx_tag (id INTEGER PRIMARY KEY, name TEXT)",
                "rx_project_tag (a INTEGER NOT NULL, b INTEGER NOT NULL, PRIMARY KEY (a, b))",
                "rx_task (id INTEGER PRIMARY KEY, project_id INTEGER, parent_id INTEGER, active INTEGER)",
                "rx_update (id INTEGER PRIMARY KEY, project_id INTEGER, date TEXT, status TEXT)",
            ];
            let rows = [
                ("rx_project", "(7, 'Seven'), (8, 'Eight')"),
                ("rx_tag", "(3, 'alpha'), (5, 'beta')"),
                ("rx_project_tag", "(7, 5), (7, 3)"),
                ("rx_task", "(1, 7, NULL, 1), (2, 7, 1, 1), (3, 7, 1, 0), (4, 8, NULL, 1), (5, 7, 2, 1)"),
                ("rx_update", "(1, 7, '2026-01-01', 'off_track'), (2, 7, '2026-02-01', 'on_track')"),
            ];
            let statements = tables
                .iter()
                .map(|t| format!("CREATE TABLE IF NOT EXISTS {}", t.replace("TEXT", TEXT)))
                .chain(rows.iter().map(|(t, v)| insert_ignore(t, v)));
            for sql in statements {
                let run = || sqlx::query(sqlx::AssertSqlSafe(sql.clone())).execute(db.pool());
                if run().await.is_err() {
                    run().await.unwrap();
                }
            }
            db
        }

        fn project(db: $db) -> T {
            Table::new("rx_project", db)
                .with_id_column("id")
                .with_column_of::<String>("name")
                .with_many("tags", "a", project_tag)
                .with_many("tasks", "project_id", task)
                .with_many("updates", "project_id", update)
        }

        fn tag(db: $db) -> T {
            Table::new("rx_tag", db)
                .with_id_column("id")
                .with_column_of::<String>("name")
        }

        fn project_tag(db: $db) -> T {
            Table::new("rx_project_tag", db)
                .with_id_column("id")
                .with_expression("id", |_| link_id())
                .with_column_of::<i64>("a")
                .with_column_of::<i64>("b")
                .with_one("tag", "b", tag)
                .with_relation_column(
                    "tag_name",
                    RelationQuery::new("tag"),
                    Aggregate::Field("name".into()),
                )
                .unwrap()
        }

        fn task(db: $db) -> T {
            Table::new("rx_task", db)
                .with_id_column("id")
                .with_column_of::<i64>("project_id")
                .with_column_of::<i64>("parent_id")
                .with_column_of::<i64>("active")
                .with_many("subtasks", "parent_id", task)
        }

        fn update(db: $db) -> T {
            Table::new("rx_update", db)
                .with_id_column("id")
                .with_column_of::<i64>("project_id")
                .with_column_of::<String>("date")
                .with_column_of::<String>("status")
        }

        fn eq(table: &T, column: &str, value: i64) -> <$db as TableSource>::Condition {
            table
                .data_source()
                .eq_value_condition(column, <$value>::from(value))
                .unwrap()
        }

        fn by_tag_name() -> RelationQuery<<$db as TableSource>::Condition> {
            RelationQuery::new("tags").with_order("tag_name", SortDirection::Ascending)
        }

        /// Column `column` of every row of `table`, as JSON, by id. A JSON
        /// value a backend returns as text reads as the value it holds.
        async fn read(table: &T, column: &str) -> std::collections::HashMap<String, JsonValue> {
            table
                .list_values()
                .await
                .unwrap()
                .into_iter()
                .map(|(id, row)| {
                    let value = JsonValue::from(row[column].clone());
                    let value = match &value {
                        JsonValue::String(text) => serde_json::from_str::<JsonValue>(text)
                            .ok()
                            .filter(JsonValue::is_array)
                            .unwrap_or(value),
                        _ => value,
                    };
                    (id, value)
                })
                .collect()
        }

        fn truthy(value: &JsonValue) -> bool {
            match value {
                JsonValue::Bool(b) => *b,
                JsonValue::Number(n) => n.as_i64() != Some(0),
                other => panic!("not a boolean: {other}"),
            }
        }

        #[tokio::test]
        async fn relation_join_json_ordered() {
            let projects = project(setup().await)
                .with_relation_column("tag_ids", by_tag_name(), Aggregate::JoinJson("b".into()))
                .unwrap();
            let ids = read(&projects, "tag_ids").await;
            assert_eq!(ids["7"], json!([3, 5]), "ordered by tag name");
            assert_eq!(ids["8"], json!([]));
        }

        #[tokio::test]
        async fn relation_join_concat_over_child_expression() {
            let projects = project(setup().await)
                .with_relation_column(
                    "tag_names",
                    by_tag_name(),
                    Aggregate::JoinConcat {
                        column: "tag_name".into(),
                        separator: ", ".into(),
                    },
                )
                .unwrap();
            let names = read(&projects, "tag_names").await;
            assert_eq!(names["7"], json!("alpha, beta"));
            assert_eq!(names["8"], json!(""));

            // Past MySQL's default 1024-byte GROUP_CONCAT limit: whole, not cut.
            let separator = "-".repeat(400);
            let tasks = project(setup().await)
                .with_relation_column(
                    "task_ids",
                    RelationQuery::new("tasks").with_order("id", SortDirection::Ascending),
                    Aggregate::JoinConcat {
                        column: "id".into(),
                        separator: separator.clone(),
                    },
                )
                .unwrap();
            let ids = read(&tasks, "task_ids").await;
            assert_eq!(ids["7"], json!(["1", "2", "3", "5"].join(&separator)));
        }

        #[tokio::test]
        async fn relation_exists_with_condition() {
            let db = setup().await;
            let links = project_tag(db.clone());
            let projects = project(db)
                .with_relation_column(
                    "has_tag_5",
                    RelationQuery::new("tags").with_condition(eq(&links, "b", 5)),
                    Aggregate::Exists,
                )
                .unwrap();
            let has = read(&projects, "has_tag_5").await;
            assert!(truthy(&has["7"]));
            assert!(!truthy(&has["8"]));

            // A raw `OR` condition stays inside the correlation: task 4 is
            // project 8's, so it mustn't count for project 7.
            let db = setup().await;
            let either = db
                .expression_condition(vantage_expressions::Expression::new(
                    "active = 0 OR id = 4",
                    vec![],
                ))
                .unwrap();
            let projects = project(db)
                .with_relation_column(
                    "odd_tasks",
                    RelationQuery::new("tasks").with_condition(either),
                    Aggregate::Count,
                )
                .unwrap();
            let counts = read(&projects, "odd_tasks").await;
            assert_eq!((&counts["7"], &counts["8"]), (&json!(1), &json!(1)));
        }

        #[tokio::test]
        async fn relation_first_by_order() {
            let projects = project(setup().await)
                .with_relation_column(
                    "last_status",
                    RelationQuery::new("updates").with_order("date", SortDirection::Descending),
                    Aggregate::First("status".into()),
                )
                .unwrap();
            let last = read(&projects, "last_status").await;
            assert_eq!(last["7"], json!("on_track"));
            assert_eq!(last["8"], JsonValue::Null);
        }

        #[tokio::test]
        async fn relation_self_reference_aliases() {
            let db = setup().await;
            let tasks = task(db.clone());
            let open = RelationQuery::new("subtasks").with_condition(eq(&tasks, "active", 1));
            let tasks = tasks
                .with_relation_column("open_subtasks", open.clone(), Aggregate::Exists)
                .unwrap()
                .with_relation_column("subtasks", RelationQuery::new("subtasks"), Aggregate::Count)
                .unwrap();
            let open = read(&tasks, "open_subtasks").await;
            let count = read(&tasks, "subtasks").await;
            assert!(truthy(&open["1"]));
            assert!(truthy(&open["2"]));
            assert!(!truthy(&open["3"]));
            assert_eq!(
                [&count["1"], &count["2"], &count["4"]],
                [&json!(2), &json!(1), &json!(0)]
            );
        }

        /// A data script narrows a vista by a relation expression: the
        /// projects with tag 5.
        #[cfg(all(feature = "vista", feature = "rhai"))]
        #[tokio::test]
        async fn vista_where_expr_exists() {
            use vantage_vista::rhai::{DataVocab, Terminals, eval_modify_script};
            let db = setup().await;
            let projects = db.vista_factory().from_table(project(db.clone())).unwrap();
            let host = vantage_rhai::Host::builder(vantage_rhai::Limits::background())
                .vocab_fn(vantage_sql::rhai_engine::relation::register_relation_vocab)
                .vocab(DataVocab::new(Terminals::Describe))
                .build();
            let tagged = eval_modify_script(
                &host,
                r#"self.where_expr(|t| t.ref("tags").where("b", 5).exists())"#,
                projects,
            )
            .unwrap();
            let ids: Vec<String> = tagged.list_values().await.unwrap().into_keys().collect();
            assert_eq!(ids, ["7"]);
        }

        /// Two levels of one self-relation: each level correlates to the
        /// row that encloses it. Task 1's grandchildren count is the sum of
        /// its subtasks' own counts (2 has 5, 3 has none).
        #[tokio::test]
        async fn relation_nested_self_reference_correlates_each_level() {
            fn counted(db: $db) -> T {
                Table::new("rx_task", db)
                    .with_id_column("id")
                    .with_column_of::<i64>("parent_id")
                    .with_many("subtasks", "parent_id", counted)
                    .with_relation_column("children", RelationQuery::new("subtasks"), Aggregate::Count)
                    .unwrap()
            }
            let tasks = counted(setup().await)
                .with_relation_column(
                    "grandchildren",
                    RelationQuery::new("subtasks"),
                    Aggregate::Sum("children".into()),
                )
                .unwrap();
            let grand = read(&tasks, "grandchildren").await;
            assert_eq!([&grand["1"], &grand["2"], &grand["5"]], [&json!(1), &json!(0), &json!(0)]);
        }

        /// A limit of 0 leaves no rows: `exists` is false, `first` null.
        #[tokio::test]
        async fn relation_limit_zero_gives_no_rows() {
            let projects = project(setup().await)
                .with_relation_column(
                    "any",
                    RelationQuery::new("tags").with_limit(0),
                    Aggregate::Exists,
                )
                .unwrap()
                .with_relation_column(
                    "first",
                    by_tag_name().with_limit(0),
                    Aggregate::First("b".into()),
                )
                .unwrap();
            assert!(!truthy(&read(&projects, "any").await["7"]));
            assert_eq!(read(&projects, "first").await["7"], JsonValue::Null);
        }

        #[tokio::test]
        async fn relation_join_without_order_is_an_error() {
            let err = project(setup().await)
                .with_relation_column(
                    "tag_ids",
                    RelationQuery::new("tags"),
                    Aggregate::JoinJson("b".into()),
                )
                .unwrap_err()
                .to_string();
            assert!(err.contains("needs order_by"), "{err}");
            assert!(err.contains("tags"), "names the relation: {err}");
        }
      }
    };
}
