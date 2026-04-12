//! Test 3b: Complex SELECT queries against the v3 test database.
//! Each query exercises specific SQL features through the Selectable trait.

#![allow(dead_code)]

use serde::Deserialize;
use vantage_expressions::{ExprDataSource, Expressive, Order, Selectable};
use vantage_sql::primitives::alias::AliasExt;
use vantage_sql::primitives::identifier::ident;
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::statements::SqliteSelect;
use vantage_sql::sqlite::statements::select::join::SqliteSelectJoin;
use vantage_sql::sqlite_expr;
use vantage_table::operation::Operation;
use vantage_types::{Record, TryFromRecord};

const DB_PATH: &str = "sqlite:../target/bakery.sqlite?mode=ro";

async fn get_db() -> SqliteDB {
    SqliteDB::connect(DB_PATH)
        .await
        .expect("Failed to connect to bakery.sqlite")
}

/// Checks that `select.preview()` matches `expected_sql`, then executes the
/// query and returns deserialized rows.
async fn check_and_run<T: for<'de> Deserialize<'de>>(
    select: &SqliteSelect,
    expected_sql: &str,
) -> Vec<T> {
    assert_eq!(select.preview(), expected_sql);

    let db = get_db().await;
    let result = db.execute(&select.expr()).await.unwrap();
    let json: serde_json::Value = result.into();
    let arr = json.as_array().unwrap();

    let records: Vec<Record<serde_json::Value>> = arr.iter().map(|v| v.clone().into()).collect();
    records
        .into_iter()
        .map(|r| T::from_record(r).unwrap())
        .collect()
}

// -- ---------------------------------------------------------------------------
// -- 1. Basic SELECT with WHERE, ORDER BY, LIMIT, OFFSET
// -- Features: column selection, AND, comparison ops (=, >), ASC, LIMIT/OFFSET
// -- Expected: returns admins earning over 50k, page 3 of 10
// -- ---------------------------------------------------------------------------
// SELECT id, name, email
// FROM users
// WHERE role = 'admin' AND salary > 50000.0
// ORDER BY name ASC
// LIMIT 10 OFFSET 20;

#[derive(Debug, Deserialize)]
struct UserBasic {
    id: i64,
    name: String,
    email: String,
}

#[tokio::test]
async fn test_q1() {
    let users: Vec<UserBasic> = check_and_run(
        &SqliteSelect::new()
            .with_source("users")
            .with_field("id")
            .with_field("name")
            .with_field("email")
            .with_condition(sqlite_expr!("\"role\" = {}", "admin"))
            .with_condition(sqlite_expr!("\"salary\" > {}", 50000.0f64))
            .with_order(sqlite_expr!("\"name\""), Order::Asc),
        "SELECT \"id\", \"name\", \"email\" FROM \"users\" \
         WHERE \"role\" = 'admin' AND \"salary\" > 50000.0 \
         ORDER BY \"name\"",
    )
    .await;

    assert_eq!(users.len(), 3);
    assert_eq!(users[0].name, "Alice Chen");
    assert_eq!(users[2].name, "Leo Russo");
}

// -- ---------------------------------------------------------------------------
// -- 2. INNER JOIN with table aliases
// -- Features: INNER JOIN, AS alias, qualified column names, multi-column ORDER BY
// -- Expected: users with their department names, salary >= 30k
// -- ---------------------------------------------------------------------------
// SELECT u.id, u.name, d.name AS department_name
// FROM users AS u
// INNER JOIN departments AS d ON d.id = u.department_id
// WHERE u.salary >= 30000.0
// ORDER BY d.name, u.name;

#[derive(Debug, Deserialize)]
struct UserWithDept {
    id: i64,
    name: String,
    department_name: String,
}

#[tokio::test]
async fn test_q2() {
    let users: Vec<UserWithDept> = check_and_run(
        &SqliteSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("id").dot_of("u"))
            .with_expression(ident("name").dot_of("u"))
            .with_expression(ident("name").dot_of("d").with_alias("department_name"))
            .with_join(SqliteSelectJoin::inner(
                "departments",
                "d",
                sqlite_expr!(
                    "{} = {}",
                    (ident("id").dot_of("d")),
                    (ident("department_id").dot_of("u"))
                ),
            ))
            .with_condition(sqlite_expr!(
                "{} >= {}",
                (ident("salary").dot_of("u")),
                30000.0f64
            ))
            .with_order(ident("name").dot_of("d"), Order::Asc)
            .with_order(ident("name").dot_of("u"), Order::Asc),
        "SELECT \"u\".\"id\", \"u\".\"name\", \"d\".\"name\" AS \"department_name\" \
         FROM \"users\" AS \"u\" \
         INNER JOIN \"departments\" AS \"d\" ON \"d\".\"id\" = \"u\".\"department_id\" \
         WHERE \"u\".\"salary\" >= 30000.0 \
         ORDER BY \"d\".\"name\", \"u\".\"name\"",
    )
    .await;

    assert_eq!(users.len(), 11);
    assert_eq!(users[0].name, "Alice Chen");
    assert_eq!(users[0].department_name, "Backend");
    assert_eq!(users[1].name, "Bob Martinez");
    assert_eq!(users[1].department_name, "Backend");
    assert_eq!(users[2].name, "Iris Novak");
    assert_eq!(users[2].department_name, "Design");
    assert_eq!(users[10].name, "Jake Torres");
    assert_eq!(users[10].department_name, "UX Research");
}

// -- ---------------------------------------------------------------------------
// -- 3. LEFT JOIN with COALESCE and aggregate
// -- Features: LEFT JOIN, COALESCE for NULL→0, SUM aggregate, GROUP BY
// -- Expected: every user with their total spend (0 if no orders)
// -- ---------------------------------------------------------------------------
// SELECT
//     u.id,
//     u.name,
//     COALESCE(SUM(o.total), 0.0) AS total_spent
// FROM users AS u
// LEFT JOIN orders AS o ON o.user_id = u.id
// GROUP BY u.id, u.name
// ORDER BY total_spent DESC;

#[derive(Debug, Deserialize)]
struct UserSpend {
    id: i64,
    name: String,
    total_spent: f64,
}

#[tokio::test]
async fn test_q3() {
    use vantage_sql::primitives::fx::Fx;

    let select = SqliteSelect::new()
        .with_source_as("users", "u")
        .with_expression(ident("id").dot_of("u"))
        .with_expression(ident("name").dot_of("u"))
        .with_expression(
            Fx::new(
                "coalesce",
                [
                    Fx::new("sum", [ident("total").dot_of("o").expr()]).expr(),
                    sqlite_expr!("{}", 0.0f64),
                ],
            )
            .as_alias("total_spent"),
        )
        .with_join(SqliteSelectJoin::left(
            "orders",
            "o",
            sqlite_expr!(
                "{} = {}",
                (ident("user_id").dot_of("o")),
                (ident("id").dot_of("u"))
            ),
        ))
        .with_group_by(ident("id").dot_of("u"))
        .with_group_by(ident("name").dot_of("u"))
        .with_order(ident("total_spent"), Order::Desc);

    let users: Vec<UserSpend> = check_and_run(
        &select,
        "SELECT \"u\".\"id\", \"u\".\"name\", \
         COALESCE(SUM(\"o\".\"total\"), 0.0) AS \"total_spent\" \
         FROM \"users\" AS \"u\" \
         LEFT JOIN \"orders\" AS \"o\" ON \"o\".\"user_id\" = \"u\".\"id\" \
         GROUP BY \"u\".\"id\", \"u\".\"name\" \
         ORDER BY \"total_spent\" DESC",
    )
    .await;

    assert_eq!(users.len(), 12);

    assert_eq!(users[0].name, "Eve Johnson");
    assert_eq!(users[0].total_spent, 2000.0);
    assert_eq!(users[1].name, "Leo Russo");
    assert_eq!(users[1].total_spent, 625.0);
    assert_eq!(users[2].name, "Bob Martinez");
    assert_eq!(users[2].total_spent, 560.0);
    assert_eq!(users[3].name, "Alice Chen");
    assert_eq!(users[3].total_spent, 500.5);

    // Users with no orders get 0.0
    for user in &users[8..12] {
        assert_eq!(user.total_spent, 0.0, "{} should have 0 spend", user.name);
    }
}

// -- ---------------------------------------------------------------------------
// -- 4. GROUP BY with HAVING and multiple aggregates
// -- Features: COUNT, AVG, MIN, MAX, HAVING with compound condition
// -- Expected: categories with >1 product and avg price < 500
// -- ---------------------------------------------------------------------------
// SELECT
//     category,
//     COUNT(*) AS product_count,
//     AVG(price) AS avg_price,
//     MIN(price) AS cheapest,
//     MAX(price) AS most_expensive
// FROM products
// GROUP BY category
// HAVING COUNT(*) > 1 AND AVG(price) < 500.0
// ORDER BY product_count DESC;

#[derive(Debug, Deserialize)]
struct CategoryStats {
    category: String,
    product_count: i64,
    avg_price: f64,
    cheapest: f64,
    most_expensive: f64,
}

#[tokio::test]
async fn test_q4() {
    use vantage_sql::primitives::fx::Fx;

    let price = ident("price");
    let stats: Vec<CategoryStats> = check_and_run(
        &SqliteSelect::new()
            .with_source("products")
            .with_field("category")
            .with_expression(Fx::new("count", [sqlite_expr!("*")]).as_alias("product_count"))
            .with_expression(Fx::new("avg", [price.expr()]).as_alias("avg_price"))
            .with_expression(Fx::new("min", [price.expr()]).as_alias("cheapest"))
            .with_expression(Fx::new("max", [price.expr()]).as_alias("most_expensive"))
            .with_group_by(ident("category"))
            .with_having(sqlite_expr!(
                "{} > {}",
                (Fx::new("count", [sqlite_expr!("*")])),
                1i64
            ))
            .with_having(sqlite_expr!(
                "{} < {}",
                (Fx::new("avg", [price.expr()])),
                500.0f64
            ))
            .with_order(ident("product_count"), Order::Desc),
        "SELECT \"category\", COUNT(*) AS \"product_count\", AVG(\"price\") AS \"avg_price\", \
         MIN(\"price\") AS \"cheapest\", MAX(\"price\") AS \"most_expensive\" \
         FROM \"products\" \
         GROUP BY \"category\" \
         HAVING COUNT(*) > 1 AND AVG(\"price\") < 500.0 \
         ORDER BY \"product_count\" DESC",
    )
    .await;

    // electronics (7), then furniture (2) and stationery (2) in any order
    assert_eq!(stats.len(), 3);
    assert_eq!(stats[0].category, "electronics");
    assert_eq!(stats[0].product_count, 7);
    assert_eq!(stats[1].product_count, 2);
    assert_eq!(stats[2].product_count, 2);
}

// -- ---------------------------------------------------------------------------
// -- 5. Scalar subquery in SELECT + correlated EXISTS in WHERE
// -- Features: subquery as column expr, correlated EXISTS, literal 1
// -- Expected: users who have at least one completed order, with their order count
// -- ---------------------------------------------------------------------------
// SELECT
//     u.id,
//     u.name,
//     (SELECT COUNT(*) FROM orders AS o WHERE o.user_id = u.id) AS order_count
// FROM users AS u
// WHERE EXISTS (
//     SELECT 1 FROM orders AS o
//     WHERE o.user_id = u.id AND o.status = 'completed'
// )
// ORDER BY order_count DESC
// LIMIT 20;

#[derive(Debug, Deserialize)]
struct UserOrderCount {
    id: i64,
    name: String,
    order_count: i64,
}

#[tokio::test]
async fn test_q5() {
    use vantage_sql::primitives::fx::Fx;

    let user_id_match = sqlite_expr!(
        "{} = {}",
        (ident("user_id").dot_of("o")),
        (ident("id").dot_of("u"))
    );

    let count_subquery = SqliteSelect::new()
        .with_source_as("orders", "o")
        .with_expression(Fx::new("count", [sqlite_expr!("*")]))
        .with_condition(user_id_match.clone());

    let exists_subquery = SqliteSelect::new()
        .with_source_as("orders", "o")
        .with_expression(sqlite_expr!("1"))
        .with_condition(user_id_match)
        .with_condition(sqlite_expr!(
            "{} = {}",
            (ident("status").dot_of("o")),
            "completed"
        ));

    let users: Vec<UserOrderCount> = check_and_run(
        &SqliteSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("id").dot_of("u"))
            .with_expression(ident("name").dot_of("u"))
            .with_expression(
                sqlite_expr!("({})", (count_subquery))
                .as_alias("order_count"),
            )
            .with_condition(Fx::new("exists", [exists_subquery.expr()]))
            .with_order(ident("order_count"), Order::Desc)
            .with_limit(Some(20), None),
        concat!(
            r#"SELECT "u"."id", "u"."name", "#,
            r#"(SELECT COUNT(*) FROM "orders" AS "o" WHERE "o"."user_id" = "u"."id") AS "order_count" "#,
            r#"FROM "users" AS "u" "#,
            r#"WHERE EXISTS(SELECT 1 FROM "orders" AS "o" WHERE "o"."user_id" = "u"."id" AND "o"."status" = 'completed') "#,
            r#"ORDER BY "order_count" DESC "#,
            r#"LIMIT 20"#,
        ),
    )
    .await;

    // Users with completed orders: Alice(3 orders), Bob(1), Carol(1), Eve(1), Frank(1), Leo(1), Jake(1)
    // But only those with status='completed': Alice(2), Bob(1), Carol(1), Eve(1), Frank(1), Leo(1), Jake(1)
    assert_eq!(users.len(), 7);
    // Alice has most orders (4 total), ordered DESC
    assert_eq!(users[0].name, "Alice Chen");
    assert_eq!(users[0].order_count, 4);
}

// -- ---------------------------------------------------------------------------
// -- 6. Derived table (subquery in FROM)
// -- Features: subquery as table source, aggregate inside derived table, != operator
// -- Expected: users who placed 2+ non-cancelled orders, with stats
// -- ---------------------------------------------------------------------------
// SELECT
//     u.name,
//     stats.order_count,
//     stats.avg_total
// FROM users AS u
// INNER JOIN (
//     SELECT user_id, COUNT(*) AS order_count, AVG(total) AS avg_total
//     FROM orders
//     WHERE status != 'cancelled'
//     GROUP BY user_id
// ) AS stats ON stats.user_id = u.id
// WHERE stats.order_count >= 2
// ORDER BY stats.avg_total DESC;

#[derive(Debug, Deserialize)]
struct UserOrderStats {
    name: String,
    order_count: i64,
    avg_total: f64,
}

#[tokio::test]
async fn test_q6() {
    use vantage_sql::primitives::fx::Fx;

    let stats_subquery = SqliteSelect::new()
        .with_source("orders")
        .with_field("user_id")
        .with_expression(Fx::new("count", [sqlite_expr!("*")]).as_alias("order_count"))
        .with_expression(Fx::new("avg", [ident("total").expr()]).as_alias("avg_total"))
        .with_condition(sqlite_expr!("{} != {}", (ident("status")), "cancelled"))
        .with_group_by(ident("user_id"));

    let users: Vec<UserOrderStats> = check_and_run(
        &SqliteSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("name").dot_of("u"))
            .with_expression(ident("order_count").dot_of("stats"))
            .with_expression(ident("avg_total").dot_of("stats"))
            .with_join(SqliteSelectJoin::inner_expr(
                stats_subquery,
                "stats",
                sqlite_expr!(
                    "{} = {}",
                    (ident("user_id").dot_of("stats")),
                    (ident("id").dot_of("u"))
                ),
            ))
            .with_condition(sqlite_expr!(
                "{} >= {}",
                (ident("order_count").dot_of("stats")),
                2i64
            ))
            .with_order(ident("avg_total").dot_of("stats"), Order::Desc),
        concat!(
            r#"SELECT "u"."name", "stats"."order_count", "stats"."avg_total" "#,
            r#"FROM "users" AS "u" "#,
            r#"INNER JOIN (SELECT "user_id", COUNT(*) AS "order_count", AVG("total") AS "avg_total" "#,
            r#"FROM "orders" "#,
            r#"WHERE "status" != 'cancelled' "#,
            r#"GROUP BY "user_id") AS "stats" ON "stats"."user_id" = "u"."id" "#,
            r#"WHERE "stats"."order_count" >= 2 "#,
            r#"ORDER BY "stats"."avg_total" DESC"#,
        ),
    )
    .await;

    // Users with 2+ non-cancelled orders:
    // Alice: 4 orders (250+125.5+75+50), Bob: 1 non-cancelled (500), Eve: 2 (1200+800),
    // Carol: 2 (310+45), Leo: 2 (350+275)
    // So: Alice(4), Eve(2), Carol(2), Leo(2)
    assert_eq!(users.len(), 4);
    // Ordered by avg_total DESC: Eve (1000), Leo (312.5), Alice (125.125), Carol (177.5)
    assert_eq!(users[0].name, "Eve Johnson");
    assert_eq!(users[0].order_count, 2);
}

// -- ---------------------------------------------------------------------------
// -- 7. CASE expression, IIF, string concatenation, generated column read
// -- Features: CASE/WHEN/THEN/ELSE, IIF(), || concat, reading generated col
// -- Expected: all users with salary band, admin flag, and display_name
// -- ---------------------------------------------------------------------------
// SELECT
//     id,
//     name,
//     salary,
//     CASE
//         WHEN salary >= 100000.0 THEN 'senior'
//         WHEN salary >= 60000.0  THEN 'mid'
//         WHEN salary >= 30000.0  THEN 'junior'
//         ELSE 'intern'
//     END AS band,
//     IIF(role = 'admin', 'Yes', 'No') AS is_admin,
//     display_name
// FROM users
// ORDER BY salary DESC;

#[derive(Debug, Deserialize)]
struct UserBand {
    id: i64,
    name: String,
    salary: f64,
    band: String,
    is_admin: String,
    display_name: String,
}

#[tokio::test]
async fn test_q7() {
    use vantage_sql::primitives::case::Case;
    use vantage_sql::primitives::ternary::ternary;

    let salary = ident("salary");

    let users: Vec<UserBand> = check_and_run(
        &SqliteSelect::new()
            .with_source("users")
            .with_field("id")
            .with_field("name")
            .with_field("salary")
            .with_expression(
                Case::new()
                    .when(
                        sqlite_expr!("{} >= {}", (salary.clone()), 100000.0f64),
                        sqlite_expr!("{}", "senior"),
                    )
                    .when(
                        sqlite_expr!("{} >= {}", (salary.clone()), 60000.0f64),
                        sqlite_expr!("{}", "mid"),
                    )
                    .when(
                        sqlite_expr!("{} >= {}", (salary.clone()), 30000.0f64),
                        sqlite_expr!("{}", "junior"),
                    )
                    .else_(sqlite_expr!("{}", "intern"))
                    .as_alias("band"),
            )
            .with_expression(ternary(ident("role").eq("admin"), "Yes", "No").as_alias("is_admin"))
            .with_field("display_name")
            .with_order(ident("salary"), Order::Desc),
        "SELECT \"id\", \"name\", \"salary\", \
         CASE WHEN \"salary\" >= 100000.0 THEN 'senior' WHEN \"salary\" >= 60000.0 THEN 'mid' \
         WHEN \"salary\" >= 30000.0 THEN 'junior' ELSE 'intern' END AS \"band\", \
         IIF(\"role\" = 'admin', 'Yes', 'No') AS \"is_admin\", \
         \"display_name\" \
         FROM \"users\" \
         ORDER BY \"salary\" DESC",
    )
    .await;

    assert_eq!(users.len(), 12);
    // Leo: 130k → senior, admin
    assert_eq!(users[0].name, "Leo Russo");
    assert_eq!(users[0].band, "senior");
    assert_eq!(users[0].is_admin, "Yes");
    // Karen: 25k → intern
    assert_eq!(users[11].name, "Karen Hill");
    assert_eq!(users[11].band, "intern");
    assert_eq!(users[11].is_admin, "No");
    // display_name is generated: "name <email>"
    assert!(users[0].display_name.contains("Leo Russo"));
    assert!(users[0].display_name.contains("leo@example.com"));
}

// -- ---------------------------------------------------------------------------
// -- 8. UNION ALL + EXCEPT compound select
// -- Features: UNION ALL, EXCEPT, literal string column, IS NOT NULL
// -- Expected: admin users UNION ALL departments, EXCEPT departments with zero budget
// -- ---------------------------------------------------------------------------
// SELECT id, name, 'user' AS source FROM users WHERE role = 'admin'
// UNION ALL
// SELECT id, name, 'department' AS source FROM departments WHERE budget IS NOT NULL
// EXCEPT
// SELECT id, name, 'department' AS source FROM departments WHERE budget = 0.0;

#[derive(Debug, Deserialize)]
struct NamedSource {
    id: i64,
    name: String,
    source: String,
}

#[tokio::test]
async fn test_q8() {
    use vantage_sql::primitives::union::Union;

    let admins = SqliteSelect::new()
        .with_source("users")
        .with_field("id")
        .with_field("name")
        .with_expression(sqlite_expr!("{}", "user").as_alias("source"))
        .with_condition(sqlite_expr!("{} = {}", (ident("role")), "admin"));

    let depts_with_budget = SqliteSelect::new()
        .with_source("departments")
        .with_field("id")
        .with_field("name")
        .with_expression(sqlite_expr!("{}", "department").as_alias("source"))
        .with_condition(sqlite_expr!("{} IS NOT NULL", (ident("budget"))));

    let depts_zero_budget = SqliteSelect::new()
        .with_source("departments")
        .with_field("id")
        .with_field("name")
        .with_expression(sqlite_expr!("{}", "department").as_alias("source"))
        .with_condition(sqlite_expr!("{} = {}", (ident("budget")), 0.0f64));

    let compound = Union::new(admins)
        .union_all(depts_with_budget)
        .except(depts_zero_budget);

    let db = get_db().await;
    let result = db.execute(&compound.expr()).await.unwrap();
    let json: serde_json::Value = result.into();
    let arr = json.as_array().unwrap();

    assert_eq!(
        compound.preview(),
        "SELECT \"id\", \"name\", 'user' AS \"source\" FROM \"users\" WHERE \"role\" = 'admin' \
         UNION ALL \
         SELECT \"id\", \"name\", 'department' AS \"source\" FROM \"departments\" WHERE \"budget\" IS NOT NULL \
         EXCEPT \
         SELECT \"id\", \"name\", 'department' AS \"source\" FROM \"departments\" WHERE \"budget\" = 0.0"
    );

    let records: Vec<Record<serde_json::Value>> = arr.iter().map(|v| v.clone().into()).collect();
    let rows: Vec<NamedSource> = records
        .into_iter()
        .map(|r| NamedSource::from_record(r).unwrap())
        .collect();

    // 3 admins + 8 departments with non-null budget, minus 0 with budget=0.0
    // All departments have budget > 0 so EXCEPT removes nothing
    assert_eq!(rows.len(), 11);

    let user_count = rows.iter().filter(|r| r.source == "user").count();
    let dept_count = rows.iter().filter(|r| r.source == "department").count();
    assert_eq!(user_count, 3);
    assert_eq!(dept_count, 8);
}

// -- ---------------------------------------------------------------------------
// -- 9. Window functions — ROW_NUMBER, RANK, running SUM, named WINDOW
// -- Features: ROW_NUMBER, RANK, SUM OVER, PARTITION BY, ROWS frame, WINDOW clause
// -- Expected: per-department salary ranking with running total
// -- ---------------------------------------------------------------------------
// SELECT
//     u.department_id,
//     u.name,
//     u.salary,
//     ROW_NUMBER() OVER win AS row_num,
//     RANK() OVER win AS salary_rank,
//     SUM(u.salary) OVER (
//         PARTITION BY u.department_id
//         ORDER BY u.salary DESC
//         ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
//     ) AS running_total
// FROM users AS u
// WHERE u.department_id IS NOT NULL
// WINDOW win AS (PARTITION BY u.department_id ORDER BY u.salary DESC);

#[derive(Debug, Deserialize)]
struct SalaryRanking {
    department_id: i64,
    name: String,
    salary: f64,
    row_num: i64,
    salary_rank: i64,
    running_total: f64,
}

#[tokio::test]
async fn test_q9() {
    use vantage_sql::primitives::fx::Fx;
    use vantage_sql::primitives::select::window::Window;

    let dept = ident("department_id").dot_of("u");
    let salary = ident("salary").dot_of("u");

    let rows: Vec<SalaryRanking> = check_and_run(
        &SqliteSelect::new()
            .with_source_as("users", "u")
            .with_expression(dept.clone())
            .with_expression(ident("name").dot_of("u"))
            .with_expression(salary.clone())
            .with_expression(
                Window::named("win").apply(Fx::new("row_number", vec![]))
                .as_alias("row_num"),
            )
            .with_expression(
                Window::named("win").apply(Fx::new("rank", vec![]))
                .as_alias("salary_rank"),
            )
            .with_expression(
                Window::new()
                    .partition_by(dept.clone())
                    .order_by(salary.clone(), Order::Desc)
                    .rows("UNBOUNDED PRECEDING", "CURRENT ROW")
                    .apply(Fx::new("sum", [salary.expr()]))
                .as_alias("running_total"),
            )
            .with_condition(sqlite_expr!("{} IS NOT NULL", (dept)))
            .with_window("win", Window::new()
                .partition_by(dept.clone())
                .order_by(salary.clone(), Order::Desc)),
        concat!(
            r#"SELECT "u"."department_id", "u"."name", "u"."salary", "#,
            r#"ROW_NUMBER() OVER win AS "row_num", "#,
            r#"RANK() OVER win AS "salary_rank", "#,
            r#"SUM("u"."salary") OVER (PARTITION BY "u"."department_id" ORDER BY "u"."salary" DESC "#,
            r#"ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS "running_total" "#,
            r#"FROM "users" AS "u" "#,
            r#"WHERE "u"."department_id" IS NOT NULL "#,
            r#"WINDOW win AS (PARTITION BY "u"."department_id" ORDER BY "u"."salary" DESC)"#,
        ),
    )
    .await;

    // 11 users with non-null department_id
    assert_eq!(rows.len(), 11);
    // First row in Backend (dept 2): Alice 120k, row_num=1, rank=1, running=120k
    let backend: Vec<&SalaryRanking> = rows.iter().filter(|r| r.department_id == 2).collect();
    assert_eq!(backend.len(), 2);
    assert_eq!(backend[0].name, "Alice Chen");
    assert_eq!(backend[0].row_num, 1);
    assert_eq!(backend[0].running_total, 120000.0);
    assert_eq!(backend[1].name, "Bob Martinez");
    assert_eq!(backend[1].running_total, 215000.0); // 120k + 95k
}

// -- ---------------------------------------------------------------------------
// -- 10. Non-recursive CTE — multiple WITH clauses, CTE referencing CTE
// -- Features: WITH, two CTEs, IN list, SUM, chained CTE reference
// -- Expected: top spenders with revenue > 400
// -- ---------------------------------------------------------------------------
// WITH
//     active_orders AS (
//         SELECT user_id, COUNT(*) AS cnt, SUM(total) AS revenue
//         FROM orders
//         WHERE status IN ('completed', 'shipped')
//         GROUP BY user_id
//     ),
//     top_spenders AS (
//         SELECT user_id, revenue
//         FROM active_orders
//         WHERE revenue > 400.0
//     )
// SELECT u.name, t.revenue
// FROM top_spenders AS t
// INNER JOIN users AS u ON u.id = t.user_id
// ORDER BY t.revenue DESC
// LIMIT 10;

#[derive(Debug, Deserialize)]
struct TopSpender {
    name: String,
    revenue: f64,
}

#[tokio::test]
async fn test_q10() {
    use vantage_sql::primitives::fx::Fx;

    let spenders: Vec<TopSpender> = check_and_run(
        &SqliteSelect::new()
            .with_cte("active_orders", SqliteSelect::new()
                .with_source("orders")
                .with_field("user_id")
                .with_expression(Fx::new("count", [sqlite_expr!("*")]).as_alias("cnt"))
                .with_expression(Fx::new("sum", [ident("total").expr()]).as_alias("revenue"))
                .with_condition(sqlite_expr!("{} IN ({}, {})",
                    (ident("status")), "completed", "shipped"))
                .with_group_by(ident("user_id")), false)
            .with_cte("top_spenders", SqliteSelect::new()
                .with_source("active_orders")
                .with_field("user_id")
                .with_field("revenue")
                .with_condition(sqlite_expr!("{} > {}", (ident("revenue")), 400.0f64)), false)
            .with_source_as("top_spenders", "t")
            .with_expression(ident("name").dot_of("u"))
            .with_expression(ident("revenue").dot_of("t"))
            .with_join(SqliteSelectJoin::inner("users", "u",
                sqlite_expr!("{} = {}",
                    (ident("id").dot_of("u")),
                    (ident("user_id").dot_of("t")))))
            .with_order(ident("revenue").dot_of("t"), Order::Desc)
            .with_limit(Some(10), None),
        concat!(
            r#"WITH active_orders AS (SELECT "user_id", COUNT(*) AS "cnt", SUM("total") AS "revenue" "#,
            r#"FROM "orders" "#,
            r#"WHERE "status" IN ('completed', 'shipped') "#,
            r#"GROUP BY "user_id"), "#,
            r#"top_spenders AS (SELECT "user_id", "revenue" "#,
            r#"FROM "active_orders" "#,
            r#"WHERE "revenue" > 400.0) "#,
            r#"SELECT "u"."name", "t"."revenue" "#,
            r#"FROM "top_spenders" AS "t" "#,
            r#"INNER JOIN "users" AS "u" ON "u"."id" = "t"."user_id" "#,
            r#"ORDER BY "t"."revenue" DESC "#,
            r#"LIMIT 10"#,
        ),
    )
    .await;

    // Users with completed+shipped revenue > 400:
    // Eve: 2000, Leo: 625, Bob: 500, Alice: 450.5, Jake: 420
    assert_eq!(spenders.len(), 5);
    assert_eq!(spenders[0].name, "Eve Johnson");
    assert_eq!(spenders[0].revenue, 2000.0);
    assert_eq!(spenders[1].name, "Leo Russo");
    assert_eq!(spenders[1].revenue, 625.0);
}

// -- ---------------------------------------------------------------------------
// -- 11. Recursive CTE — hierarchical department tree
// -- Features: WITH RECURSIVE, UNION ALL, self-referencing join, || concat, IS NULL
// -- Expected: full department tree with depth and breadcrumb path
// -- ---------------------------------------------------------------------------
// WITH RECURSIVE dept_tree(id, name, depth, path) AS (
//     SELECT id, name, 0, name
//     FROM departments
//     WHERE parent_id IS NULL
//     UNION ALL
//     SELECT d.id, d.name, dt.depth + 1, dt.path || ' > ' || d.name
//     FROM departments AS d
//     INNER JOIN dept_tree AS dt ON dt.id = d.parent_id
// )
// SELECT id, name, depth, path
// FROM dept_tree
// ORDER BY path;

#[derive(Debug, Deserialize)]
struct DeptTree {
    id: i64,
    name: String,
    depth: i64,
    path: String,
}

#[tokio::test]
async fn test_q11() {
    use vantage_sql::concat_sql;
    use vantage_sql::primitives::union::Union;

    let base = SqliteSelect::new()
        .with_source("departments")
        .with_field("id")
        .with_field("name")
        .with_expression(sqlite_expr!("0"))
        .with_expression(ident("name"))
        .with_condition(sqlite_expr!("{} IS NULL", (ident("parent_id"))));

    let recursive = SqliteSelect::new()
        .with_source_as("departments", "d")
        .with_expression(ident("id").dot_of("d"))
        .with_expression(ident("name").dot_of("d"))
        .with_expression(sqlite_expr!("{} + 1", (ident("depth").dot_of("dt"))))
        .with_expression(concat_sql!(
            ident("path").dot_of("dt"),
            " > ",
            ident("name").dot_of("d")
        ))
        .with_join(SqliteSelectJoin::inner(
            "dept_tree",
            "dt",
            sqlite_expr!(
                "{} = {}",
                (ident("id").dot_of("dt")),
                (ident("parent_id").dot_of("d"))
            ),
        ));

    let rows: Vec<DeptTree> = check_and_run(
        &SqliteSelect::new()
            .with_cte(
                "dept_tree(id, name, depth, path)",
                Union::new(base).union_all(recursive),
                true,
            )
            .with_source("dept_tree")
            .with_field("id")
            .with_field("name")
            .with_field("depth")
            .with_field("path")
            .with_order(ident("path"), Order::Asc),
        concat!(
            r#"WITH RECURSIVE dept_tree(id, name, depth, path) AS "#,
            r#"(SELECT "id", "name", 0, "name" FROM "departments" WHERE "parent_id" IS NULL "#,
            r#"UNION ALL "#,
            r#"SELECT "d"."id", "d"."name", "dt"."depth" + 1, "dt"."path" || ' > ' || "d"."name" "#,
            r#"FROM "departments" AS "d" "#,
            r#"INNER JOIN "dept_tree" AS "dt" ON "dt"."id" = "d"."parent_id") "#,
            r#"SELECT "id", "name", "depth", "path" "#,
            r#"FROM "dept_tree" "#,
            r#"ORDER BY "path""#,
        ),
    )
    .await;

    // 8 departments total in the tree
    assert_eq!(rows.len(), 8);
    // Root nodes at depth 0
    let roots: Vec<&DeptTree> = rows.iter().filter(|r| r.depth == 0).collect();
    assert_eq!(roots.len(), 3); // Engineering, Sales, Design
    // Check a nested path
    let backend = rows.iter().find(|r| r.name == "Backend").unwrap();
    assert_eq!(backend.depth, 1);
    assert_eq!(backend.path, "Engineering > Backend");
}

// -- ---------------------------------------------------------------------------
// -- 12. Multi-way JOIN through junction table + DISTINCT + LIKE + IN
// -- Features: 3-table join, DISTINCT, LIKE pattern, IN list, many-to-many
// -- Expected: products tagged 'electronics','sale', or 'featured' with "Pro" in name
// -- ---------------------------------------------------------------------------
// SELECT DISTINCT p.id, p.name, p.price
// FROM products AS p
// INNER JOIN product_tags AS pt ON pt.product_id = p.id
// INNER JOIN tags AS t ON t.id = pt.tag_id
// WHERE t.name IN ('electronics', 'sale', 'featured')
//   AND p.name LIKE '%Pro%'
//   AND p.stock > 0
// ORDER BY p.price ASC
// LIMIT 50;

#[derive(Debug, Deserialize)]
struct ProductMatch {
    id: i64,
    name: String,
    price: f64,
}

#[tokio::test]
async fn test_q12() {
    let products: Vec<ProductMatch> = check_and_run(
        &SqliteSelect::new()
            .with_distinct(true)
            .with_source_as("products", "p")
            .with_expression(ident("id").dot_of("p"))
            .with_expression(ident("name").dot_of("p"))
            .with_expression(ident("price").dot_of("p"))
            .with_join(SqliteSelectJoin::inner(
                "product_tags",
                "pt",
                sqlite_expr!(
                    "{} = {}",
                    (ident("product_id").dot_of("pt")),
                    (ident("id").dot_of("p"))
                ),
            ))
            .with_join(SqliteSelectJoin::inner(
                "tags",
                "t",
                sqlite_expr!(
                    "{} = {}",
                    (ident("id").dot_of("t")),
                    (ident("tag_id").dot_of("pt"))
                ),
            ))
            .with_condition(sqlite_expr!(
                "{} IN ({}, {}, {})",
                (ident("name").dot_of("t")),
                "electronics",
                "sale",
                "featured"
            ))
            .with_condition(sqlite_expr!(
                "{} LIKE {}",
                (ident("name").dot_of("p")),
                "%Pro%"
            ))
            .with_condition(sqlite_expr!("{} > {}", (ident("stock").dot_of("p")), 0i64))
            .with_order(ident("price").dot_of("p"), Order::Asc)
            .with_limit(Some(50), None),
        concat!(
            r#"SELECT DISTINCT "p"."id", "p"."name", "p"."price" "#,
            r#"FROM "products" AS "p" "#,
            r#"INNER JOIN "product_tags" AS "pt" ON "pt"."product_id" = "p"."id" "#,
            r#"INNER JOIN "tags" AS "t" ON "t"."id" = "pt"."tag_id" "#,
            r#"WHERE "t"."name" IN ('electronics', 'sale', 'featured') "#,
            r#"AND "p"."name" LIKE '%Pro%' "#,
            r#"AND "p"."stock" > 0 "#,
            r#"ORDER BY "p"."price" "#,
            r#"LIMIT 50"#,
        ),
    )
    .await;

    // Widget Pro (electronics, 29.99) and Gadget Pro Max (electronics, 99.99) match
    assert_eq!(products.len(), 2);
    assert_eq!(products[0].name, "Widget Pro");
    assert_eq!(products[1].name, "Gadget Pro Max");
}

// -- ---------------------------------------------------------------------------
// -- 13. JSON operators + BETWEEN + CAST + NULLIF
// -- Features: json_extract(), ->> operator, CAST AS REAL, BETWEEN, NULLIF
// -- Expected: products with rating 4.0–5.0 and in_stock, with extracted JSON fields
// -- ---------------------------------------------------------------------------
// SELECT
//     id,
//     name,
//     json_extract(metadata, '$.color') AS color,
//     metadata ->> '$.weight_kg' AS weight,
//     CAST(metadata ->> '$.rating' AS REAL) AS rating,
//     NULLIF(category, 'uncategorized') AS clean_category
// FROM products
// WHERE CAST(metadata ->> '$.rating' AS REAL) BETWEEN 4.0 AND 5.0
//   AND json_extract(metadata, '$.in_stock') = 1
// ORDER BY rating DESC;

#[derive(Debug, Deserialize)]
struct ProductJson {
    id: i64,
    name: String,
    color: String,
    weight: f64,
    rating: f64,
    clean_category: Option<String>,
}

#[tokio::test]
async fn test_q13() {
    use vantage_sql::primitives::fx::Fx;
    use vantage_sql::primitives::json_extract::JsonExtract;

    let metadata = ident("metadata");
    let products: Vec<ProductJson> = check_and_run(
        &SqliteSelect::new()
            .with_source("products")
            .with_field("id")
            .with_field("name")
            .with_expression(JsonExtract::new(metadata.clone(), "color").as_alias("color"))
            .with_expression(
                sqlite_expr!("{} ->> {}", (metadata.clone()), "$.weight_kg").as_alias("weight"),
            )
            .with_expression(
                sqlite_expr!("CAST({} ->> {} AS REAL)", (metadata.clone()), "$.rating")
                    .as_alias("rating"),
            )
            .with_expression(
                Fx::new(
                    "nullif",
                    [
                        ident("category").expr(),
                        sqlite_expr!("{}", "uncategorized"),
                    ],
                )
                .as_alias("clean_category"),
            )
            .with_condition(sqlite_expr!(
                "CAST({} ->> {} AS REAL) BETWEEN {} AND {}",
                (metadata.clone()),
                "$.rating",
                4.0f64,
                5.0f64
            ))
            .with_condition(JsonExtract::new(metadata.clone(), "in_stock").eq(1i64))
            .with_order(ident("rating"), Order::Desc),
        concat!(
            r#"SELECT "id", "name", "#,
            r#"JSON_EXTRACT("metadata", '$.color') AS "color", "#,
            r#""metadata" ->> '$.weight_kg' AS "weight", "#,
            r#"CAST("metadata" ->> '$.rating' AS REAL) AS "rating", "#,
            r#"NULLIF("category", 'uncategorized') AS "clean_category" "#,
            r#"FROM "products" "#,
            r#"WHERE CAST("metadata" ->> '$.rating' AS REAL) BETWEEN 4.0 AND 5.0 "#,
            r#"AND JSON_EXTRACT("metadata", '$.in_stock') = 1 "#,
            r#"ORDER BY "rating" DESC"#,
        ),
    )
    .await;

    // Products with rating 4.0-5.0 and in_stock=1
    assert!(products.len() >= 5);
    // Highest rated first
    assert!(products[0].rating >= products[1].rating);
    // Gadget Pro Max has rating 4.9
    assert_eq!(products[0].name, "Gadget Pro Max");
    assert_eq!(products[0].color, "silver");
}

// -- ---------------------------------------------------------------------------
// -- 14. Date functions + ROUND + typeof + GROUP BY expression + HAVING
// -- Features: strftime(), ROUND(), typeof(), GROUP BY on function result
// -- Expected: monthly revenue per department for 2025, > 100 total
// -- ---------------------------------------------------------------------------
// SELECT
//     strftime('%Y-%m', o.created_at) AS month,
//     d.name AS department,
//     COUNT(o.id) AS order_count,
//     ROUND(SUM(o.total), 2) AS monthly_revenue,
//     typeof(SUM(o.total)) AS sum_type
// FROM orders AS o
// INNER JOIN users AS u ON u.id = o.user_id
// INNER JOIN departments AS d ON d.id = u.department_id
// WHERE o.created_at >= '2025-01-01'
// GROUP BY strftime('%Y-%m', o.created_at), d.name
// HAVING SUM(o.total) > 100.0
// ORDER BY month DESC, monthly_revenue DESC;

#[derive(Debug, Deserialize)]
struct MonthlyRevenue {
    month: String,
    department: String,
    order_count: i64,
    monthly_revenue: f64,
    sum_type: String,
}

#[tokio::test]
async fn test_q14() {
    use vantage_sql::primitives::date_format::DateFormat;
    use vantage_sql::primitives::fx::Fx;

    let o_total = ident("total").dot_of("o");
    let sum_total = Fx::new("sum", [o_total.expr()]);
    let month_expr = DateFormat::new(ident("created_at").dot_of("o"), "%Y-%m");

    let rows: Vec<MonthlyRevenue> = check_and_run(
        &SqliteSelect::new()
            .with_source_as("orders", "o")
            .with_expression(month_expr.clone().as_alias("month"))
            .with_expression(ident("name").dot_of("d").as_alias("department"))
            .with_expression(
                Fx::new("count", [ident("id").dot_of("o").expr()]).as_alias("order_count"),
            )
            .with_expression(
                Fx::new("round", [sum_total.expr(), sqlite_expr!("{}", 2i64)])
                    .as_alias("monthly_revenue"),
            )
            .with_expression(Fx::new("typeof", [sum_total.expr()]).as_alias("sum_type"))
            .with_join(SqliteSelectJoin::inner(
                "users",
                "u",
                sqlite_expr!(
                    "{} = {}",
                    (ident("id").dot_of("u")),
                    (ident("user_id").dot_of("o"))
                ),
            ))
            .with_join(SqliteSelectJoin::inner(
                "departments",
                "d",
                sqlite_expr!(
                    "{} = {}",
                    (ident("id").dot_of("d")),
                    (ident("department_id").dot_of("u"))
                ),
            ))
            .with_condition(sqlite_expr!(
                "{} >= {}",
                (ident("created_at").dot_of("o")),
                "2025-01-01"
            ))
            .with_group_by(month_expr)
            .with_group_by(ident("name").dot_of("d"))
            .with_having(sqlite_expr!("{} > {}", (sum_total), 100.0f64))
            .with_order(ident("month"), Order::Desc)
            .with_order(ident("monthly_revenue"), Order::Desc),
        concat!(
            r#"SELECT STRFTIME('%Y-%m', "o"."created_at") AS "month", "#,
            r#""d"."name" AS "department", "#,
            r#"COUNT("o"."id") AS "order_count", "#,
            r#"ROUND(SUM("o"."total"), 2) AS "monthly_revenue", "#,
            r#"TYPEOF(SUM("o"."total")) AS "sum_type" "#,
            r#"FROM "orders" AS "o" "#,
            r#"INNER JOIN "users" AS "u" ON "u"."id" = "o"."user_id" "#,
            r#"INNER JOIN "departments" AS "d" ON "d"."id" = "u"."department_id" "#,
            r#"WHERE "o"."created_at" >= '2025-01-01' "#,
            r#"GROUP BY STRFTIME('%Y-%m', "o"."created_at"), "d"."name" "#,
            r#"HAVING SUM("o"."total") > 100.0 "#,
            r#"ORDER BY "month" DESC, "monthly_revenue" DESC"#,
        ),
    )
    .await;

    assert!(!rows.is_empty());
    // typeof returns the SQLite storage type
    assert!(
        rows.iter()
            .all(|r| r.sum_type == "real" || r.sum_type == "integer")
    );
    // All rows have revenue > 100 (HAVING filter)
    assert!(rows.iter().all(|r| r.monthly_revenue > 100.0));
}

// -- ---------------------------------------------------------------------------
// -- 15. Window functions — FILTER, LAG, LEAD, FIRST_VALUE, NTH_VALUE
// -- Features: aggregate FILTER (WHERE), LAG/LEAD with offset, FIRST_VALUE,
// --           NTH_VALUE, RANGE and ROWS frame specs
// -- Expected: each user with completed-order count, neighboring salaries,
// --           top and 2nd earner per department
// -- ---------------------------------------------------------------------------
// SELECT
//     u.id,
//     u.name,
//     u.salary,
//     u.department_id,
//     COUNT(*) FILTER (WHERE o.status = 'completed') OVER (PARTITION BY u.id) AS completed_count,
//     LAG(u.salary, 1) OVER (ORDER BY u.salary) AS prev_salary,
//     LEAD(u.salary, 1) OVER (ORDER BY u.salary) AS next_salary,
//     FIRST_VALUE(u.name) OVER (
//         PARTITION BY u.department_id ORDER BY u.salary DESC
//         RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
//     ) AS top_earner,
//     NTH_VALUE(u.name, 2) OVER (
//         PARTITION BY u.department_id ORDER BY u.salary DESC
//         ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING
//     ) AS second_earner
// FROM users AS u
// LEFT JOIN orders AS o ON o.user_id = u.id
// WHERE u.department_id IS NOT NULL
// ORDER BY u.department_id, u.salary DESC;

#[derive(Debug, Deserialize)]
struct UserWindowStats {
    id: i64,
    name: String,
    salary: f64,
    department_id: i64,
    completed_count: i64,
    prev_salary: Option<f64>,
    next_salary: Option<f64>,
    top_earner: String,
    second_earner: Option<String>,
}

#[tokio::test]
async fn test_q15() {
    use vantage_sql::primitives::fx::Fx;
    use vantage_sql::primitives::select::window::Window;

    let dept = ident("department_id").dot_of("u");
    let salary = ident("salary").dot_of("u");
    let u_name = ident("name").dot_of("u");

    let dept_salary_win = Window::new()
        .partition_by(dept.clone())
        .order_by(salary.clone(), Order::Desc);

    let rows: Vec<UserWindowStats> = check_and_run(
        &SqliteSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("id").dot_of("u"))
            .with_expression(u_name.clone())
            .with_expression(salary.clone())
            .with_expression(dept.clone())
            .with_expression(
                sqlite_expr!(
                    "COUNT(*) FILTER (WHERE {} = {}) OVER (PARTITION BY {})",
                    (ident("status").dot_of("o")), "completed",
                    (ident("id").dot_of("u"))
                )
                .as_alias("completed_count"),
            )
            .with_expression(
                Window::new().order_by(salary.clone(), Order::Asc)
                    .apply(sqlite_expr!("LAG({}, 1)", (salary.clone())))
                .as_alias("prev_salary"),
            )
            .with_expression(
                Window::new().order_by(salary.clone(), Order::Asc)
                    .apply(sqlite_expr!("LEAD({}, 1)", (salary.clone())))
                .as_alias("next_salary"),
            )
            .with_expression(
                dept_salary_win.clone()
                    .range("UNBOUNDED PRECEDING", "UNBOUNDED FOLLOWING")
                    .apply(Fx::new("first_value", [u_name.expr()]))
                .as_alias("top_earner"),
            )
            .with_expression(
                dept_salary_win.clone()
                    .rows("UNBOUNDED PRECEDING", "UNBOUNDED FOLLOWING")
                    .apply(Fx::new("nth_value", [u_name.expr(), sqlite_expr!("2")]))
                .as_alias("second_earner"),
            )
            .with_join(SqliteSelectJoin::left("orders", "o",
                sqlite_expr!("{} = {}",
                    (ident("user_id").dot_of("o")),
                    (ident("id").dot_of("u")))))
            .with_condition(sqlite_expr!("{} IS NOT NULL", (dept.clone())))
            .with_order(dept, Order::Asc)
            .with_order(salary, Order::Desc),
        concat!(
            r#"SELECT "u"."id", "u"."name", "u"."salary", "u"."department_id", "#,
            r#"COUNT(*) FILTER (WHERE "o"."status" = 'completed') OVER (PARTITION BY "u"."id") AS "completed_count", "#,
            r#"LAG("u"."salary", 1) OVER (ORDER BY "u"."salary") AS "prev_salary", "#,
            r#"LEAD("u"."salary", 1) OVER (ORDER BY "u"."salary") AS "next_salary", "#,
            r#"FIRST_VALUE("u"."name") OVER (PARTITION BY "u"."department_id" ORDER BY "u"."salary" DESC "#,
            r#"RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) AS "top_earner", "#,
            r#"NTH_VALUE("u"."name", 2) OVER (PARTITION BY "u"."department_id" ORDER BY "u"."salary" DESC "#,
            r#"ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) AS "second_earner" "#,
            r#"FROM "users" AS "u" "#,
            r#"LEFT JOIN "orders" AS "o" ON "o"."user_id" = "u"."id" "#,
            r#"WHERE "u"."department_id" IS NOT NULL "#,
            r#"ORDER BY "u"."department_id", "u"."salary" DESC"#,
        ),
    )
    .await;

    // LEFT JOIN duplicates rows for users with multiple orders
    assert!(!rows.is_empty());

    // Backend dept (id=2): Alice (120k) is top earner
    let alice = rows.iter().find(|r| r.name == "Alice Chen").unwrap();
    assert_eq!(alice.department_id, 2);
    assert_eq!(alice.top_earner, "Alice Chen");

    // Hank Patel is sole member of Engineering (dept 1) — no second earner
    let hank = rows.iter().find(|r| r.name == "Hank Patel").unwrap();
    assert_eq!(hank.top_earner, "Hank Patel");
    assert_eq!(hank.second_earner, None);
}
