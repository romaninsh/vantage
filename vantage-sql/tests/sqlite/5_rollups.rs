//! Roll-up columns: `with_rollup` over a has_many relation, rendered as a
//! correlated subquery, checked against hand-computed values from
//! bakery.sqlite.
//!
//! order_line prices: order1 → 120, 135, 199; order2 → 220; order3 → 199.
//! Clients: marty has order1, doc has order2 and order3, biff has none.

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

/// `employee.with_many("reports", "manager_id", employee)`: an unaliased
/// subquery reads `employee.manager_id = employee.id` against its own row and
/// counts the wrong rows. The roll-up must count right or refuse to build.
#[tokio::test]
async fn a_self_referencing_rollup_never_gives_wrong_counts() {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    for sql in [
        "CREATE TABLE employee (id TEXT PRIMARY KEY, name TEXT, manager_id TEXT)",
        "INSERT INTO employee VALUES ('boss', 'Boss', NULL), ('ann', 'Ann', 'boss'), \
         ('bob', 'Bob', 'boss'), ('cat', 'Cat', 'ann')",
    ] {
        sqlx::query(sql).execute(db.pool()).await.unwrap();
    }
    fn employee(db: SqliteDB) -> Table<SqliteDB, vantage_types::EmptyEntity> {
        Table::new("employee", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("manager_id")
            .with_many("reports", "manager_id", employee)
    }
    let Ok(employees) =
        employee(db.clone()).with_rollup("reports", "reports", RollupOp::Count, None, None)
    else {
        return;
    };
    let rows = employees.list_values().await.unwrap();
    let count = |id: &str| JsonValue::from(rows[id]["reports"].clone());
    assert_eq!(
        [count("boss"), count("ann"), count("bob")],
        [json!(2), json!(1), json!(0)]
    );
}

/// A missing column, an unknown column, an unknown relation and a computed
/// child column are build errors. A computed column would aggregate the stored
/// field of the same name, not the expression.
#[tokio::test]
async fn bad_rollups_error_at_build() {
    let db = get_db().await;
    let no_column = order_table(db.clone()).with_rollup("t", "lines", RollupOp::Sum, None, None);
    assert!(no_column.is_err());
    let unknown_column =
        order_table(db.clone()).with_rollup("t", "lines", RollupOp::Max, Some("nope"), None);
    assert!(unknown_column.is_err());
    let unknown_relation =
        order_table(db.clone()).with_rollup("t", "nope", RollupOp::Count, None, None);
    assert!(unknown_relation.is_err());
    fn doubled_lines(db: SqliteDB) -> Table<SqliteDB, OrderLine> {
        line_table(db).with_expression("price", |_| sqlite_expr!("price * 2"))
    }
    let computed = order_table(db)
        .with_many("doubled", "order_id", doubled_lines)
        .with_rollup("t", "doubled", RollupOp::Sum, Some("price"), None);
    assert!(computed.is_err());
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
/// row) has no clear set to aggregate: refused at build.
#[tokio::test]
async fn rollup_over_a_paginated_child_is_refused() {
    fn second_page(db: SqliteDB) -> Table<SqliteDB, OrderLine> {
        let mut lines = line_table(db);
        lines.set_pagination(Some(Pagination::new(2, 1)));
        lines
    }
    let built = order_table(get_db().await)
        .with_many("paged", "order_id", second_page)
        .with_rollup("t", "paged", RollupOp::Count, None, None);
    assert!(built.is_err());
}
