//! The `expr:` vocabulary: `sql::json`, and `table.ref(…)` relation
//! expressions building the same SQL as the Rust API.

use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Env, Host, Limits};
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::vista::rhai_source::{eval_column_expr, register_sqlite_onto};
use vantage_table::sorting::SortDirection;
use vantage_table::table::{Aggregate, RelationQuery, Table};
use vantage_types::EmptyEntity;
use vantage_vista::rhai::{DataVocab, Terminals};

fn project(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::new("rx_project", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("tags", "a", project_tag)
}

fn project_tag(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
    Table::new("rx_project_tag", db)
        .with_id_column("id")
        .with_column_of::<i64>("a")
        .with_column_of::<i64>("b")
        .with_column_of::<String>("tag_name")
}

async fn db() -> SqliteDB {
    SqliteDB::connect("sqlite::memory:").await.unwrap()
}

/// `p.n` sums `t.m`, and `t.m` reads `p.n` back: a cycle, refused with a
/// named error instead of building until the stack runs out.
#[tokio::test]
async fn relation_columns_reading_each_other_are_a_named_error() {
    fn p(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
        Table::new("rx_p", db)
            .with_id_column("id")
            .with_many("ts", "p_id", t)
            .with_relation_column("n", RelationQuery::new("ts"), Aggregate::Sum("m".into()))
            .unwrap()
    }
    fn t(db: SqliteDB) -> Table<SqliteDB, EmptyEntity> {
        Table::new("rx_t", db)
            .with_id_column("id")
            .with_column_of::<String>("p_id")
            .with_one("p", "p_id", p)
            .with_relation_column("m", RelationQuery::new("p"), Aggregate::Field("n".into()))
            .unwrap()
    }
    let err = p(db().await).check_expressions().unwrap_err().to_string();
    assert!(err.contains("reads itself through its relations"), "{err}");
    assert!(err.contains("rx_p"), "names the table: {err}");
}

#[test]
fn rhai_sql_json_renders_per_dialect() {
    let text = r#"sql::json(sql::ident("name"), "en_US")"#;
    let sqlite = |code| eval_column_expr(code, None, Env::new()).unwrap().preview();
    assert_eq!(sqlite(text), "JSON_EXTRACT(\"name\", '$.en_US')");
    #[cfg(feature = "postgres")]
    {
        let raw = r#"sql::json_raw(sql::ident("name"), "en_US")"#;
        let postgres = |code| {
            vantage_sql::postgres::vista::rhai_source::eval_column_expr(code, None, Env::new())
                .unwrap()
                .preview()
        };
        assert_eq!(postgres(text), "\"name\" ->> 'en_US'");
        assert_eq!(postgres(raw), "\"name\" -> 'en_US'");
        let path = r#"sql::json(sql::ident("meta"), ["specs", "volts"])"#;
        assert_eq!(postgres(path), "\"meta\" -> 'specs' ->> 'volts'");
    }
    #[cfg(feature = "mysql")]
    {
        let mysql = |code| {
            vantage_sql::mysql::vista::rhai_source::eval_column_expr(code, None, Env::new())
                .unwrap()
                .preview()
        };
        assert_eq!(mysql(text), "JSON_UNQUOTE(JSON_EXTRACT(`name`, '$.en_US'))");
        // MySQL reads `en-US` or `1st` bare as a broken path; they're quoted.
        let dashed = r#"sql::json(sql::ident("name"), ["en-US", "1st"])"#;
        assert_eq!(
            mysql(dashed),
            "JSON_UNQUOTE(JSON_EXTRACT(`name`, '$.\"en-US\".\"1st\"'))"
        );
    }
    // A key is written into the SQL, so anything but a plain name is refused.
    let quoted = r#"sql::json(sql::ident("meta"), "a' OR 1=1 --")"#;
    let err = eval_column_expr(quoted, None, Env::new())
        .unwrap_err()
        .to_string();
    assert!(err.contains("path keys are letters"), "{err}");
}

#[tokio::test]
async fn rhai_relation_chain_builds_expression() {
    let table = project(db().await);
    let script = r#"table.ref("tags").order_by("tag_name").join_json("b")"#;
    let from_script = eval_column_expr(script, Some(table.clone()), Env::new()).unwrap();
    let from_rust = table
        .relation_expression(
            &RelationQuery::new("tags").with_order("tag_name", SortDirection::Ascending),
            &Aggregate::JoinJson("b".into()),
        )
        .unwrap();
    assert_eq!(from_script.preview(), from_rust.preview());

    let counted = eval_column_expr(
        r#"table.ref("tags").where("b", 5).exists()"#,
        Some(table),
        Env::new(),
    )
    .unwrap()
    .preview();
    assert!(
        counted.starts_with("EXISTS (SELECT 1 FROM \"rx_project_tag\""),
        "{counted}"
    );
    assert!(counted.ends_with("AND b = 5)"), "{counted}");
}

#[tokio::test]
async fn rhai_relation_unknown_relation_errors() {
    let err = eval_column_expr(
        r#"table.ref("tagz").count()"#,
        Some(project(db().await)),
        Env::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("tagz"), "names the relation: {err}");
    assert!(err.contains("rx_project"), "names the table: {err}");
}

/// A `where` column the child doesn't have is an error naming it, not a
/// reference SQL would resolve to the outer table's column of that name. An
/// empty value (an unset `session.uid`) is refused rather than widening.
#[tokio::test]
async fn rhai_relation_unknown_where_column_errors() {
    let err = eval_column_expr(
        r#"table.ref("tags").where("name", "x").count()"#,
        Some(project(db().await)),
        Env::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("no column `name`"), "{err}");
    assert!(
        err.contains("rx_project_tag"),
        "names the child table: {err}"
    );
    let empty = eval_column_expr(
        r#"table.ref("tags").where("b", ()).count()"#,
        Some(project(db().await)),
        Env::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        empty.contains("where(b): the value is empty (()); use null() to match NULL"),
        "{empty}"
    );
}

/// A hook-style script narrowing a data handle by a query expression, as a
/// line copied from an `expr:` would: refused, naming the verb.
#[test]
fn rhai_vocab_refuses_expression_as_value() {
    let host = Host::builder(Limits::background())
        .vocab_fn(register_sqlite_onto)
        .vocab(DataVocab::new(Terminals::Describe))
        .build();
    let env = Env::new().var(
        "self",
        Dynamic::from(vantage_vista::rhai::Handle::named("rx_project")),
    );
    let script = host
        .compile(&vantage_rhai::Block::from(
            r#"self.where("name", sql::json(sql::ident("name"), "en_US"))"#,
        ))
        .unwrap();
    let err = script.eval(&env).unwrap_err().to_string();
    assert!(err.contains("`where` takes a value"), "{err}");
    assert!(err.contains("query expression"), "{err}");
}
