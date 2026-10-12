//! Rhai DSL golden suite: every `tests/rhai-tests/*.rhai` script against its
//! pinned `.sql` output or `.err` message, once per SQL dialect. A golden
//! that no longer matches the renderer fails here.

#[cfg(feature = "rhai")]
#[path = "common/rhai_golden.rs"]
mod golden;

#[cfg(feature = "rhai")]
fn assert_goldens(vendor: &str, eval: impl Fn(&str) -> Result<String, String>) {
    let mismatches = golden::mismatches(vendor, eval);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n\n"));
}

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

    #[test]
    fn every_fixture_matches_its_golden() {
        super::assert_goldens("sqlite", |code| {
            __host()
                .compile(&vantage_rhai::Block::from(code))
                .and_then(|script| script.eval_as::<Select>(&vantage_rhai::Env::new()))
                .map(|select| select.inner.preview())
                .map_err(|e| e.to_string())
        });
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

    #[test]
    fn every_fixture_matches_its_golden() {
        super::assert_goldens("postgres", |code| {
            __host()
                .compile(&vantage_rhai::Block::from(code))
                .and_then(|script| script.eval_as::<Select>(&vantage_rhai::Env::new()))
                .map(|select| select.inner.preview())
                .map_err(|e| e.to_string())
        });
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

    #[test]
    fn every_fixture_matches_its_golden() {
        super::assert_goldens("mysql", |code| {
            __host()
                .compile(&vantage_rhai::Block::from(code))
                .and_then(|script| script.eval_as::<Select>(&vantage_rhai::Env::new()))
                .map(|select| select.inner.preview())
                .map_err(|e| e.to_string())
        });
    }
}
