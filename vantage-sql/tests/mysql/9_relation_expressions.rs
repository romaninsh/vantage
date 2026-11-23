//! Expression ids, relation expressions and `sql::concat` on MySQL: see
//! `common/expression_ids.rs`, `common/relation_aggregates.rs` and
//! `common/rhai_concat.rs`.

use vantage_expressions::Expression;
#[allow(unused_imports)]
use vantage_sql::mysql::operation::MysqlOperation;
use vantage_sql::mysql::{AnyMysqlType, MysqlDB, mysql_ident};
use vantage_sql::mysql_expr;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

async fn connect() -> MysqlDB {
    MysqlDB::connect("mysql://vantage:vantage@localhost:3306/vantage")
        .await
        .unwrap()
}

fn quote(name: &str) -> String {
    format!("`{name}`")
}

const TEXT: &str = "VARCHAR(64)";

fn insert_ignore(table: &str, values: &str) -> String {
    format!("INSERT IGNORE INTO {table} VALUES {values}")
}

fn link_id() -> Expression<AnyMysqlType> {
    mysql_expr!(
        "CONCAT({}, ':', {})",
        (mysql_ident("a")),
        (mysql_ident("b"))
    )
}

expression_ids_tests!(MysqlDB, AnyMysqlType);
relation_aggregates_tests!(MysqlDB, AnyMysqlType);
rhai_concat_tests!(
    MysqlDB,
    AnyMysqlType,
    vantage_sql::mysql::statements::MysqlSelect,
    vantage_sql::mysql::statements::select::join::MysqlSelectJoin,
    vantage_sql::condition::MysqlCondition,
    "CONCAT(`a`, {}, `b`)"
);
