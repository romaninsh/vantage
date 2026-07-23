//! Rhai DSL test suite
//!
//! Tests .rhai files against pinned .sql output for each database backend.

#[cfg(all(feature = "sqlite", feature = "rhai"))]
mod sqlite_tests {
    use vantage_sql::condition::SqliteCondition;
    use vantage_sql::rhai_engine::RhaiSelect;
    use vantage_sql::sqlite::AnySqliteType;
    use vantage_sql::sqlite::statements::SqliteSelect;
    use vantage_sql::sqlite::statements::select::join::SqliteSelectJoin;

    vantage_sql::register_engine!(
        value: AnySqliteType,
        select: SqliteSelect,
        join: SqliteSelectJoin,
        cond: SqliteCondition,
    );

    type Select = RhaiSelect<AnySqliteType, SqliteSelect, SqliteSelectJoin, SqliteCondition>;

    fn eval_rhai_file(path: &str) -> Select {
        let code = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        let engine = __create_engine();
        engine
            .eval::<Select>(&code)
            .unwrap_or_else(|e| panic!("Rhai eval failed for {}: {}", path, e))
    }

    #[test]
    fn q1_basic_query() {
        let result = eval_rhai_file("tests/rhai-tests/q1.rhai");
        let sql = result.inner.preview();
        let expected = std::fs::read_to_string("tests/rhai-tests/sqlite/q1.sql")
            .expect("Failed to read sqlite/q1.sql");
        // The pinned fixtures are pretty-printed; `preview()` renders one
        // line. SQL is whitespace-insensitive — compare tokens.
        let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(normalize(&sql), normalize(&expected));
    }
}

#[cfg(all(feature = "postgres", feature = "rhai"))]
mod postgres_tests {
    use vantage_sql::condition::PostgresCondition;
    use vantage_sql::postgres::AnyPostgresType;
    use vantage_sql::postgres::statements::PostgresSelect;
    use vantage_sql::postgres::statements::select::join::PostgresSelectJoin;
    use vantage_sql::rhai_engine::RhaiSelect;

    vantage_sql::register_engine!(
        value: AnyPostgresType,
        select: PostgresSelect,
        join: PostgresSelectJoin,
        cond: PostgresCondition,
    );

    type Select =
        RhaiSelect<AnyPostgresType, PostgresSelect, PostgresSelectJoin, PostgresCondition>;

    fn eval_rhai_file(path: &str) -> Select {
        let code = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        let engine = __create_engine();
        engine
            .eval::<Select>(&code)
            .unwrap_or_else(|e| panic!("Rhai eval failed for {}: {}", path, e))
    }

    #[test]
    fn q1_basic_query() {
        let result = eval_rhai_file("tests/rhai-tests/q1.rhai");
        let sql = result.inner.preview();
        let expected = std::fs::read_to_string("tests/rhai-tests/postgres/q1.sql")
            .expect("Failed to read postgres/q1.sql");
        // The pinned fixtures are pretty-printed; `preview()` renders one
        // line. SQL is whitespace-insensitive — compare tokens.
        let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(normalize(&sql), normalize(&expected));
    }
}

#[cfg(all(feature = "mysql", feature = "rhai"))]
mod mysql_tests {
    use vantage_sql::condition::MysqlCondition;
    use vantage_sql::mysql::AnyMysqlType;
    use vantage_sql::mysql::statements::MysqlSelect;
    use vantage_sql::mysql::statements::select::join::MysqlSelectJoin;
    use vantage_sql::rhai_engine::RhaiSelect;

    vantage_sql::register_engine!(
        value: AnyMysqlType,
        select: MysqlSelect,
        join: MysqlSelectJoin,
        cond: MysqlCondition,
    );

    type Select = RhaiSelect<AnyMysqlType, MysqlSelect, MysqlSelectJoin, MysqlCondition>;

    fn eval_rhai_file(path: &str) -> Select {
        let code = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
        let engine = __create_engine();
        engine
            .eval::<Select>(&code)
            .unwrap_or_else(|e| panic!("Rhai eval failed for {}: {}", path, e))
    }

    #[test]
    fn q1_basic_query() {
        let result = eval_rhai_file("tests/rhai-tests/q1.rhai");
        let sql = result.inner.preview();
        let expected = std::fs::read_to_string("tests/rhai-tests/mysql/q1.sql")
            .expect("Failed to read mysql/q1.sql");
        // The pinned fixtures are pretty-printed; `preview()` renders one
        // line. SQL is whitespace-insensitive — compare tokens.
        let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(normalize(&sql), normalize(&expected));
    }
}
