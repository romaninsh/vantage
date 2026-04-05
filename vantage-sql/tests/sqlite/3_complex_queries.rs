//! Test 3b: Complex SELECT queries against the v3 test database.
//! Each query exercises specific SQL features through the Selectable trait.

use serde::Deserialize;
use vantage_expressions::{ExprDataSource, Expressive, Selectable};
use vantage_sql::sqlite::statements::SqliteSelect;
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite_expr;
use vantage_types::{Record, TryFromRecord};

const DB_PATH: &str = "sqlite:../target/bakery.sqlite?mode=ro";

async fn get_db() -> SqliteDB {
    SqliteDB::connect(DB_PATH)
        .await
        .expect("Failed to connect to bakery.sqlite")
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

#[test]
fn test_q1_render() {
    let select = SqliteSelect::new()
        .with_source("users")
        .with_field("id")
        .with_field("name")
        .with_field("email")
        .with_condition(sqlite_expr!("\"role\" = {}", "admin"))
        .with_condition(sqlite_expr!("\"salary\" > {}", 50000.0f64))
        .with_order(sqlite_expr!("\"name\""), true)
        .with_limit(Some(10), Some(20));

    assert_eq!(
        select.preview(),
        "SELECT \"id\", \"name\", \"email\" FROM \"users\" WHERE \"role\" = \"admin\" AND \"salary\" > 50000.0 ORDER BY \"name\" LIMIT 10 OFFSET 20"
    );
}

#[tokio::test]
async fn test_q1_execute() {
    let db = get_db().await;

    // All admins earning > 50k, ordered by name
    let select = SqliteSelect::new()
        .with_source("users")
        .with_field("id")
        .with_field("name")
        .with_field("email")
        .with_condition(sqlite_expr!("\"role\" = {}", "admin"))
        .with_condition(sqlite_expr!("\"salary\" > {}", 50000.0f64))
        .with_order(sqlite_expr!("\"name\""), true);

    let result = db.execute(&select.expr()).await.unwrap();
    let rows = result.into_value();
    let arr = rows.as_array().unwrap();

    // v3 data: Alice (120k), Eve (110k), Leo (130k) — all admins > 50k
    assert_eq!(arr.len(), 3);

    let records: Vec<Record<serde_json::Value>> =
        arr.iter().map(|v| v.clone().into()).collect();
    let users: Vec<UserBasic> = records
        .into_iter()
        .map(|r| UserBasic::from_record(r).unwrap())
        .collect();

    // Ordered by name ASC
    assert_eq!(users[0].name, "Alice Chen");
    assert_eq!(users[1].name, "Eve Johnson");
    assert_eq!(users[2].name, "Leo Russo");

    // LIMIT 2 OFFSET 2 → only Leo
    let select_page = SqliteSelect::new()
        .with_source("users")
        .with_field("id")
        .with_field("name")
        .with_field("email")
        .with_condition(sqlite_expr!("\"role\" = {}", "admin"))
        .with_condition(sqlite_expr!("\"salary\" > {}", 50000.0f64))
        .with_order(sqlite_expr!("\"name\""), true)
        .with_limit(Some(2), Some(2));

    let record: Record<serde_json::Value> = db
        .associate(select_page.expr())
        .get()
        .await
        .unwrap();
    let user: UserBasic = UserBasic::from_record(record).unwrap();
    assert_eq!(user.name, "Leo Russo");
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
