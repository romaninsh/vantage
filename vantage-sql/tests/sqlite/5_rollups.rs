//! Roll-up columns: `with_rollup` over a has_many relation, rendered as a
//! correlated subquery, checked against hand-computed values from
//! bakery.sqlite.
//!
//! order_line prices: order1 → 120, 135, 199; order2 → 220; order3 → 199.
//! Clients: marty has order1, doc has order2 and order3, biff has none.

use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value as JsonValue, json};
use vantage_dataset::traits::ReadableValueSet;
use vantage_sql::condition::SqliteCondition;
#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB, sqlite_ident};
use vantage_sql::sqlite_expr;
use vantage_table::pagination::Pagination;
use vantage_table::table::{RollupOp, Table};
use vantage_types::entity;
use vantage_vista::JoinKey;

const DB_PATH: &str = "sqlite:../target/bakery.sqlite?mode=ro";

async fn get_db() -> SqliteDB {
    SqliteDB::connect(DB_PATH)
        .await
        .expect("Failed to connect to bakery.sqlite — run scripts/sqlite/ingress.sh first")
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Client {
    name: String,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ClientOrder {
    client_id: String,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct OrderLine {
    order_id: String,
    price: i64,
}

fn client_table(db: SqliteDB) -> Table<SqliteDB, Client> {
    Table::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_many("orders", "client_id", order_table)
}

fn order_table(db: SqliteDB) -> Table<SqliteDB, ClientOrder> {
    Table::new("client_order", db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
        .with_many("lines", "order_id", line_table)
}

fn line_table(db: SqliteDB) -> Table<SqliteDB, OrderLine> {
    Table::new("order_line", db)
        .with_id_column("id")
        .with_column_of::<String>("order_id")
        .with_column_of::<i64>("price")
}

/// Every order with one roll-up column per op over its lines, `condition`
/// applied to the lines.
fn orders_with_rollups(
    db: SqliteDB,
    condition: impl Fn() -> Option<SqliteCondition>,
) -> Table<SqliteDB, ClientOrder> {
    let ops = [
        ("lines", RollupOp::Count, None),
        ("total", RollupOp::Sum, Some("price")),
        ("cheapest", RollupOp::Min, Some("price")),
        ("dearest", RollupOp::Max, Some("price")),
        ("average", RollupOp::Avg, Some("price")),
    ];
    ops.into_iter()
        .fold(order_table(db), |orders, (name, op, column)| {
            orders
                .with_rollup(name, "lines", op, column, condition())
                .unwrap()
        })
}

async fn rollups(orders: &Table<SqliteDB, ClientOrder>, id: &str) -> JsonValue {
    let rows = orders.list_values().await.unwrap();
    let row = &rows[id];
    let get = |name: &str| JsonValue::from(row[name].clone());
    json!({
        "lines": get("lines"),
        "total": get("total"),
        "cheapest": get("cheapest"),
        "dearest": get("dearest"),
        "average": get("average"),
    })
}

#[tokio::test]
async fn rollup_ops_over_all_children() {
    let orders = orders_with_rollups(get_db().await, || None);

    assert_eq!(
        rollups(&orders, "order1").await,
        json!({"lines": 3, "total": 454, "cheapest": 120, "dearest": 199,
               "average": 454.0 / 3.0})
    );
    assert_eq!(
        rollups(&orders, "order2").await,
        json!({"lines": 1, "total": 220, "cheapest": 220, "dearest": 220, "average": 220.0})
    );
}

/// `price > 200` leaves order1 and order3 with no lines: count and sum are 0,
/// min, max and avg are null.
#[tokio::test]
async fn rollup_ops_with_a_child_condition() {
    let orders = orders_with_rollups(get_db().await, || {
        Some(sqlite_expr!("{} > {}", (sqlite_ident("price")), 200i64).into())
    });

    assert_eq!(
        rollups(&orders, "order2").await,
        json!({"lines": 1, "total": 220, "cheapest": 220, "dearest": 220, "average": 220.0})
    );
    assert_eq!(
        rollups(&orders, "order1").await,
        json!({"lines": 0, "total": 0, "cheapest": null, "dearest": null, "average": null})
    );
}

/// biff has no orders at all: the empty set without any condition.
#[tokio::test]
async fn rollup_over_an_empty_child_set() {
    let clients = client_table(get_db().await)
        .with_rollup("orders", "orders", RollupOp::Count, None, None)
        .unwrap();
    let rows = clients.list_values().await.unwrap();
    let count = |id: &str| JsonValue::from(rows[id]["orders"].clone());
    assert_eq!(count("doc"), json!(2));
    assert_eq!(count("biff"), json!(0));
}

/// One correlated subquery per roll-up column; the sum is wrapped so an empty
/// set reads 0.
#[tokio::test]
async fn rollup_renders_a_correlated_subquery() {
    let orders = order_table(get_db().await)
        .with_rollup("total", "lines", RollupOp::Sum, Some("price"), None)
        .unwrap();
    assert_eq!(
        orders.select().preview(),
        "SELECT \"id\", \"client_id\", (COALESCE((SELECT SUM(price) FROM \"order_line\" \
         WHERE \"order_line\".\"order_id\" = \"client_order\".\"id\"), 0)) AS \"total\" \
         FROM \"client_order\""
    );
}

/// A relation joined by `keys:` correlates the child's key column with the
/// parent's own key column, not the parent's id.
#[tokio::test]
async fn rollup_over_a_keyed_relation_correlates_its_keys() {
    let clients = Table::<SqliteDB, Client>::new("client", get_db().await)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .try_with_many_keyed(
            "orders",
            &[JoinKey::new("name", "client_id")],
            "client_order",
            |db| Ok(order_table(db)),
        )
        .unwrap()
        .with_rollup("orders", "orders", RollupOp::Count, None, None)
        .unwrap();
    assert_eq!(
        clients.select().preview(),
        "SELECT \"id\", \"name\", ((SELECT COUNT(*) FROM \"client_order\" \
         WHERE \"client_order\".\"client_id\" = \"client\".\"name\")) AS \"orders\" \
         FROM \"client\""
    );
}

/// boss manages ann and bob, ann manages cat; pay is 100, 50, 40 and 30.
async fn staff() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    for sql in [
        "CREATE TABLE employee (id TEXT PRIMARY KEY, name TEXT, manager_id TEXT, pay INTEGER)",
        "INSERT INTO employee VALUES ('boss', 'Boss', NULL, 100), ('ann', 'Ann', 'boss', 50), \
         ('bob', 'Bob', 'boss', 40), ('cat', 'Cat', 'ann', 30)",
    ] {
        sqlx::query(sql).execute(db.pool()).await.unwrap();
    }
    db
}

fn employee(db: SqliteDB) -> Table<SqliteDB, vantage_types::EmptyEntity> {
    Table::new("employee", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("manager_id")
        .with_column_of::<i64>("pay")
        .with_many("reports", "manager_id", employee)
}

/// `employee.with_many("reports", "manager_id", employee)`: unaliased, the
/// subquery would read `employee.manager_id = employee.id` against its own
/// row. The child is read under an alias, so each row counts its reports.
#[tokio::test]
async fn a_self_referencing_rollup_never_gives_wrong_counts() {
    let employees = employee(staff().await)
        .with_rollup("reports", "reports", RollupOp::Count, None, None)
        .unwrap()
        .with_rollup("reports_pay", "reports", RollupOp::Sum, Some("pay"), None)
        .unwrap();
    assert_eq!(
        employees.try_select().unwrap().preview(),
        "SELECT \"id\", \"name\", \"manager_id\", \"pay\", \
         ((SELECT COUNT(*) FROM \"employee\" AS \"employee_rollup\" \
         WHERE \"employee_rollup\".\"manager_id\" = \"employee\".\"id\")) AS \"reports\", \
         (COALESCE((SELECT SUM(pay) FROM \"employee\" AS \"employee_rollup\" \
         WHERE \"employee_rollup\".\"manager_id\" = \"employee\".\"id\"), 0)) AS \"reports_pay\" \
         FROM \"employee\""
    );

    let rows = employees.list_values().await.unwrap();
    let read = |id: &str, column: &str| JsonValue::from(rows[id][column].clone());
    assert_eq!(
        [
            read("boss", "reports"),
            read("ann", "reports"),
            read("bob", "reports")
        ],
        [json!(2), json!(1), json!(0)]
    );
    assert_eq!(
        [
            read("boss", "reports_pay"),
            read("ann", "reports_pay"),
            read("bob", "reports_pay")
        ],
        [json!(90), json!(30), json!(0)]
    );
}

/// Over its own table a column that aggregates itself would read itself
/// without end: a named error from the read and from the check, at once.
#[tokio::test]
async fn a_rollup_column_aggregating_itself_is_a_named_error() {
    let employees = employee(staff().await)
        .with_rollup("pay", "reports", RollupOp::Sum, Some("pay"), None)
        .unwrap();
    for error in [
        employees.check_expressions().unwrap_err(),
        employees.list_values().await.unwrap_err(),
    ] {
        assert_eq!(error.message(), "a roll-up column can't aggregate itself");
        assert_eq!(error.context["expression_column"], "pay");
        assert_eq!(error.context["expression_table"], "employee");
    }
}

/// The error a roll-up gives when its column is read, from each way in: the
/// check a factory runs on a finished table, the select, and the read.
async fn query_time_error(table: &Table<SqliteDB, ClientOrder>) -> String {
    let checked = table.check_expressions().unwrap_err();
    let selected = table.try_select().unwrap_err();
    let read = table.list_values().await.unwrap_err();
    assert_eq!(checked.message(), selected.message());
    assert_eq!(checked.message(), read.message());
    assert_eq!(read.context["expression_column"], "t");
    read.message().to_string()
}

/// What needs no other table is refused when the column is added: an unknown
/// relation, an op without its column. What the child decides is refused
/// when the column is read: an unknown child column. A computed child column
/// is aggregated by its expression.
#[tokio::test]
async fn bad_rollups_error_at_build_or_when_read() {
    let db = get_db().await;
    let no_column = order_table(db.clone()).with_rollup("t", "lines", RollupOp::Sum, None, None);
    assert!(no_column.is_err());
    let unknown_relation =
        order_table(db.clone()).with_rollup("t", "nope", RollupOp::Count, None, None);
    assert!(unknown_relation.is_err());

    let unknown_column = order_table(db.clone())
        .with_rollup("t", "lines", RollupOp::Max, Some("nope"), None)
        .unwrap();
    assert_eq!(
        query_time_error(&unknown_column).await,
        "roll-up column not found"
    );
    fn doubled_lines(db: SqliteDB) -> Table<SqliteDB, OrderLine> {
        line_table(db).with_expression("price", |_| sqlite_expr!("price * 2"))
    }
    let computed = order_table(db)
        .with_many("doubled", "order_id", doubled_lines)
        .with_rollup("t", "doubled", RollupOp::Sum, Some("price"), None)
        .unwrap();
    let rows = computed.list_values().await.unwrap();
    assert_eq!(JsonValue::from(rows["order1"]["t"].clone()), json!(908));
}

/// A has_one relation has no child set to roll up: refused at build.
#[tokio::test]
async fn rollup_over_a_has_one_relation_is_refused() {
    let built = order_table(get_db().await)
        .with_one("client", "client_id", client_table)
        .with_rollup("t", "client", RollupOp::Count, None, None);
    assert!(built.is_err());
}

/// A has_many target with a window (an offset would skip the one aggregate
/// row) has no clear set to aggregate: refused when the column is read.
#[tokio::test]
async fn rollup_over_a_paginated_child_is_refused() {
    fn second_page(db: SqliteDB) -> Table<SqliteDB, OrderLine> {
        let mut lines = line_table(db);
        lines.set_pagination(Some(Pagination::new(2, 1)));
        lines
    }
    let paged = order_table(get_db().await)
        .with_many("paged", "order_id", second_page)
        .with_rollup("t", "paged", RollupOp::Count, None, None)
        .unwrap();
    assert_eq!(
        query_time_error(&paged).await,
        "a roll-up's relation target can't have pagination"
    );
}

/// A roll-up builds its child table when a query includes the column, and
/// only then: not when it is added, and not for a select or a count that
/// leaves the column out.
#[tokio::test]
async fn a_rollup_outside_the_active_columns_builds_no_table() {
    static BUILDS: AtomicUsize = AtomicUsize::new(0);
    fn counted_lines(db: SqliteDB) -> Table<SqliteDB, OrderLine> {
        BUILDS.fetch_add(1, Ordering::SeqCst);
        line_table(db)
    }
    let orders = order_table(get_db().await)
        .with_many("counted", "order_id", counted_lines)
        .with_rollup("total", "counted", RollupOp::Sum, Some("price"), None)
        .unwrap()
        .with_active_columns(&["id", "client_id"])
        .unwrap();

    orders.try_select().unwrap();
    assert_eq!(orders.get_count().await.unwrap(), 3);
    assert_eq!(BUILDS.load(Ordering::SeqCst), 0);

    let with_total = orders.with_active_columns(&["total"]).unwrap();
    let rows = with_total.list_values().await.unwrap();
    assert_eq!(JsonValue::from(rows["order1"]["total"].clone()), json!(454));
    assert!(BUILDS.load(Ordering::SeqCst) >= 1);
}
