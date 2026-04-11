//! MySQL-specific query builder tests.
//! These exercise features that only exist in MySQL (GROUP_CONCAT, JSON_TABLE, etc.).
//! Runs against the vantage_v4_my database (v4_my.sql schema).

#![allow(dead_code)]

use serde::Deserialize;
use vantage_expressions::{ExprDataSource, Expression, Expressive, Order, Selectable};
use vantage_sql::mysql::MysqlDB;
use vantage_sql::mysql::operation::MysqlOperation;
use vantage_sql::mysql::statements::MysqlSelect;
use vantage_sql::mysql::statements::primitives::GroupConcat;
use vantage_sql::mysql::statements::select::join::MysqlSelectJoin;
use vantage_sql::mysql_expr;
use vantage_sql::primitives::concat::Concat;
use vantage_sql::primitives::fx::Fx;
use vantage_sql::primitives::identifier::ident;
use vantage_sql::primitives::iif::Iif;
use vantage_table::operation::Operation;
use vantage_types::{Record, TryFromRecord};

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage_v4_my";

async fn get_db() -> MysqlDB {
    MysqlDB::connect(MYSQL_URL)
        .await
        .expect("Failed to connect to vantage_v4_my")
}

/// Checks that `select.preview()` matches `expected_sql`, then executes the
/// query and returns deserialized rows.
async fn check_and_run<T: for<'de> Deserialize<'de>>(
    select: &MysqlSelect,
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
// -- 1. GROUP_CONCAT — MySQL-only aggregate (PG has string_agg, not this syntax)
// -- Features: GROUP_CONCAT with DISTINCT, ORDER BY, SEPARATOR
// -- Expected: per-category comma-separated product names, semicolon-separated prices
// -- ---------------------------------------------------------------------------
// SELECT
//     p.category,
//     COUNT(*) AS total,
//     GROUP_CONCAT(DISTINCT p.name ORDER BY p.name SEPARATOR ', ') AS product_names,
//     GROUP_CONCAT(p.price ORDER BY p.price DESC SEPARATOR '; ') AS prices_desc
// FROM products AS p
// GROUP BY p.category
// ORDER BY total DESC;

#[derive(Debug, Deserialize)]
struct CategoryAgg {
    category: String,
    total: i64,
    product_names: String,
    prices_desc: String,
}

#[tokio::test]
async fn test_my_q1_group_concat() {
    let rows: Vec<CategoryAgg> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("products", "p")
            .with_expression(ident("category").dot_of("p"), None)
            .with_expression(
                Fx::new("count", [mysql_expr!("*")]),
                Some("total".into()),
            )
            .with_expression(
                GroupConcat::new(ident("name").dot_of("p"))
                    .distinct()
                    .order_by(ident("name").dot_of("p"), Order::Asc)
                    .separator(", "),
                Some("product_names".into()),
            )
            .with_expression(
                GroupConcat::new(ident("price").dot_of("p"))
                    .order_by(ident("price").dot_of("p"), Order::Desc)
                    .separator("; "),
                Some("prices_desc".into()),
            )
            .with_group_by(ident("category").dot_of("p"))
            .with_order(ident("total"), Order::Desc),
        concat!(
            "SELECT `p`.`category`, ",
            "COUNT(*) AS `total`, ",
            "GROUP_CONCAT(DISTINCT `p`.`name` ORDER BY `p`.`name` SEPARATOR ', ') AS `product_names`, ",
            "GROUP_CONCAT(`p`.`price` ORDER BY `p`.`price` DESC SEPARATOR '; ') AS `prices_desc`",
            " FROM `products` AS `p`",
            " GROUP BY `p`.`category`",
            " ORDER BY `total` DESC",
        ),
    )
    .await;

    // electronics has the most products (7)
    assert_eq!(rows[0].category, "electronics");
    assert_eq!(rows[0].total, 7);
    assert!(rows[0].product_names.contains("Widget Pro"));
    assert!(rows.len() >= 4); // electronics, home, furniture, stationery, uncategorized
}

// -- -----------------------------------------------------------------------------
// -- 2. ENUM / SET column operations + FIND_IN_SET
// -- Features: ENUM comparison, FIND_IN_SET() for SET column, FIELD() for ordering
// -- Expected: users with write permission, ordered by role's enum ordinal
// -- -----------------------------------------------------------------------------
// SELECT
//     u.id,
//     u.name,
//     u.role,
//     u.permissions,
//     FIND_IN_SET('write', u.permissions) AS has_write_at_pos,
//     FIND_IN_SET('admin', u.permissions) AS has_admin_at_pos
// FROM users AS u
// WHERE FIND_IN_SET('write', u.permissions) > 0
// ORDER BY FIELD(u.role, 'admin', 'editor', 'viewer'), u.salary DESC;

#[derive(Debug, Deserialize)]
struct UserPermissions {
    id: i64,
    name: String,
    role: String,
    permissions: String,
    has_write_at_pos: i64,
    has_admin_at_pos: i64,
}

#[tokio::test]
async fn test_my_q2_find_in_set() {
    let find_write = Fx::new(
        "find_in_set",
        [
            mysql_expr!("{}", "write"),
            ident("permissions").dot_of("u").expr(),
        ],
    );
    let find_admin = Fx::new(
        "find_in_set",
        [
            mysql_expr!("{}", "admin"),
            ident("permissions").dot_of("u").expr(),
        ],
    );

    let rows: Vec<UserPermissions> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("id").dot_of("u"), None)
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("role").dot_of("u"), None)
            .with_expression(ident("permissions").dot_of("u"), None)
            .with_expression(find_write.clone(), Some("has_write_at_pos".into()))
            .with_expression(find_admin.clone(), Some("has_admin_at_pos".into()))
            .with_condition(find_write.gt(mysql_expr!("{}", 0i64)))
            .with_order(
                Fx::new(
                    "field",
                    [
                        ident("role").dot_of("u").expr(),
                        mysql_expr!("'admin'"),
                        mysql_expr!("'editor'"),
                        mysql_expr!("'viewer'"),
                    ],
                ),
                Order::Asc,
            )
            .with_order(ident("salary").dot_of("u"), Order::Desc),
        concat!(
            "SELECT `u`.`id`, `u`.`name`, `u`.`role`, `u`.`permissions`, ",
            "FIND_IN_SET('write', `u`.`permissions`) AS `has_write_at_pos`, ",
            "FIND_IN_SET('admin', `u`.`permissions`) AS `has_admin_at_pos`",
            " FROM `users` AS `u`",
            " WHERE FIND_IN_SET('write', `u`.`permissions`) > 0",
            " ORDER BY FIELD(`u`.`role`, 'admin', 'editor', 'viewer'), `u`.`salary` DESC",
        ),
    )
    .await;

    // All returned users should have 'write' permission
    for row in &rows {
        assert!(row.has_write_at_pos > 0);
        assert!(row.permissions.contains("write"));
    }
    // Admins (with write) should come first
    assert_eq!(rows[0].role, "admin");
}

// -- -----------------------------------------------------------------------------
// -- 3. JSON operators — ->, ->>, JSON_EXTRACT, JSON_CONTAINS, MEMBER OF
// -- Features: -> (returns JSON), ->> (returns text), JSON_CONTAINS(),
// --           value MEMBER OF (json_array), JSON_UNQUOTE
// -- Expected: electronics with black color and rating >= 4.0
// -- -----------------------------------------------------------------------------
// SELECT
//     p.id,
//     p.name,
//     p.metadata->>'$.color' AS color,
//     p.metadata->>'$.rating' AS rating,
//     p.metadata->'$.specs' AS specs_json,
//     JSON_UNQUOTE(JSON_EXTRACT(p.metadata, '$.specs.voltage')) AS voltage
// FROM products AS p
// WHERE JSON_CONTAINS(p.metadata, '"black"', '$.color')
//   AND CAST(p.metadata->>'$.rating' AS DECIMAL(3,1)) >= 4.0
// ORDER BY CAST(p.metadata->>'$.rating' AS DECIMAL(3,1)) DESC;

#[derive(Debug, Deserialize)]
struct ProductJson {
    id: i64,
    name: String,
    color: Option<String>,
    rating: Option<String>,
    specs_json: Option<serde_json::Value>,
    voltage: Option<String>,
}

#[tokio::test]
async fn test_my_q3_json_operators() {
    use vantage_sql::primitives::json_extract::JsonExtract;

    let metadata = ident("metadata").dot_of("p");
    let color = JsonExtract::new(metadata.clone(), "color");
    let rating = JsonExtract::new(metadata.clone(), "rating");
    let specs = JsonExtract::new(metadata.clone(), "specs").as_json();
    let rating_cast = rating.clone().cast("DECIMAL(3,1)");

    let rows: Vec<ProductJson> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("products", "p")
            .with_expression(ident("id").dot_of("p"), None)
            .with_expression(ident("name").dot_of("p"), None)
            .with_expression(color, Some("color".into()))
            .with_expression(rating.clone(), Some("rating".into()))
            .with_expression(specs, Some("specs_json".into()))
            .with_expression(
                Fx::new(
                    "json_unquote",
                    [Fx::new(
                        "json_extract",
                        [metadata.expr(), mysql_expr!("'$.specs.voltage'")],
                    )
                    .expr()],
                ),
                Some("voltage".into()),
            )
            .with_condition(Fx::new(
                "json_contains",
                [
                    metadata.expr(),
                    mysql_expr!("'\"black\"'"),
                    mysql_expr!("'$.color'"),
                ],
            ))
            .with_condition(rating_cast.clone().gte(mysql_expr!("{}", 4.0f64)))
            .with_order(rating_cast, Order::Desc),
        concat!(
            "SELECT `p`.`id`, `p`.`name`, ",
            "`p`.`metadata` ->> '$.color' AS `color`, ",
            "`p`.`metadata` ->> '$.rating' AS `rating`, ",
            "`p`.`metadata` -> '$.specs' AS `specs_json`, ",
            "JSON_UNQUOTE(JSON_EXTRACT(`p`.`metadata`, '$.specs.voltage')) AS `voltage`",
            " FROM `products` AS `p`",
            " WHERE JSON_CONTAINS(`p`.`metadata`, '\"black\"', '$.color')",
            " AND CAST(`p`.`metadata` ->> '$.rating' AS DECIMAL(3,1)) >= 4.0",
            " ORDER BY CAST(`p`.`metadata` ->> '$.rating' AS DECIMAL(3,1)) DESC",
        ),
    )
    .await;

    // Should return Widget Pro (black, rating 4.7)
    // Should return 3 black products with rating >= 4.0
    assert_eq!(rows.len(), 3);
    for row in &rows {
        assert_eq!(row.color.as_deref(), Some("black"));
    }
}

// -- -----------------------------------------------------------------------------
// -- 4. JSON_TABLE — turn JSON into relational rows (MySQL 8.0+, not in PG/SQLite)
// -- Features: JSON_TABLE(), COLUMNS(), PATH, nested JSON extraction
// -- Expected: products with specs extracted as columns via JSON_TABLE
// -- -----------------------------------------------------------------------------
// SELECT
//     p.id,
//     p.name,
//     specs.voltage,
//     specs.watts
// FROM products AS p,
// JSON_TABLE(
//     p.metadata,
//     '$' COLUMNS (
//         voltage INT PATH '$.specs.voltage' DEFAULT '0' ON EMPTY,
//         watts   INT PATH '$.specs.watts'   DEFAULT '0' ON EMPTY
//     )
// ) AS specs
// WHERE specs.voltage > 0
// ORDER BY specs.watts DESC;

#[derive(Debug, Deserialize)]
struct ProductSpecs {
    id: i64,
    name: String,
    voltage: i64,
    watts: i64,
}

#[tokio::test]
async fn test_my_q4_json_table() {
    use vantage_sql::mysql::statements::primitives::{JsonTable, JsonTableColumn};

    let rows: Vec<ProductSpecs> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("products", "p")
            .with_source_as(
                JsonTable::new(ident("metadata").dot_of("p"))
                    .column(
                        JsonTableColumn::new("voltage", "INT", "$.specs.voltage")
                            .default("0")
                            .on_empty(),
                    )
                    .column(
                        JsonTableColumn::new("watts", "INT", "$.specs.watts")
                            .default("0")
                            .on_empty(),
                    )
                    .expr(),
                "specs",
            )
            .with_expression(ident("id").dot_of("p"), None)
            .with_expression(ident("name").dot_of("p"), None)
            .with_expression(ident("voltage").dot_of("specs"), None)
            .with_expression(ident("watts").dot_of("specs"), None)
            .with_condition(ident("voltage").dot_of("specs").gt(mysql_expr!("{}", 0i64)))
            .with_order(ident("watts").dot_of("specs"), Order::Desc),
        concat!(
            "SELECT `p`.`id`, `p`.`name`, `specs`.`voltage`, `specs`.`watts`",
            " FROM `products` AS `p`, ",
            "JSON_TABLE(`p`.`metadata`, '$' COLUMNS (",
            "voltage INT PATH '$.specs.voltage' DEFAULT '0' ON EMPTY, ",
            "watts INT PATH '$.specs.watts' DEFAULT '0' ON EMPTY",
            ")) AS `specs`",
            " WHERE `specs`.`voltage` > 0",
            " ORDER BY `specs`.`watts` DESC",
        ),
    )
    .await;

    // Products with voltage > 0: Widget Pro (10W), Widget Basic (5W),
    // Gadget Pro Max (25W), Monitor 27" (45W)
    assert_eq!(rows.len(), 4);
    // Ordered by watts DESC — Monitor should be first
    assert_eq!(rows[0].name, "Monitor 27\"");
    assert_eq!(rows[0].watts, 45);
}

// -- -----------------------------------------------------------------------------
// -- 5. JSON_ARRAYAGG / JSON_OBJECTAGG — MySQL-only JSON aggregation
// -- Features: JSON_ARRAYAGG(), JSON_OBJECTAGG(), building JSON from rows
// -- Expected: per-department: array of user names, object of name→salary
// -- -----------------------------------------------------------------------------
// SELECT
//     d.name AS department,
//     JSON_ARRAYAGG(u.name ORDER BY u.name) AS user_names,
//     JSON_OBJECTAGG(u.name, u.salary) AS salary_map
// FROM departments AS d
// INNER JOIN users AS u ON u.department_id = d.id
// WHERE u.is_active = b'1'
// GROUP BY d.id, d.name
// ORDER BY d.name;

#[derive(Debug, Deserialize)]
struct DeptJsonAgg {
    department: String,
    user_names: serde_json::Value,
    salary_map: serde_json::Value,
}

#[tokio::test]
async fn test_my_q5_json_agg() {
    let u_name = ident("name").dot_of("u");
    let u_salary = ident("salary").dot_of("u");

    let rows: Vec<DeptJsonAgg> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("departments", "d")
            .with_join(MysqlSelectJoin::inner(
                "users",
                "u",
                ident("department_id")
                    .dot_of("u")
                    .eq(ident("id").dot_of("d")),
            ))
            .with_expression(ident("name").dot_of("d"), Some("department".into()))
            .with_expression(
                Fx::new("json_arrayagg", [u_name.expr()]),
                Some("user_names".into()),
            )
            .with_expression(
                Fx::new("json_objectagg", [u_name.expr(), u_salary.expr()]),
                Some("salary_map".into()),
            )
            .with_condition(mysql_expr!("{} = b'1'", (ident("is_active").dot_of("u"))))
            .with_group_by(ident("id").dot_of("d"))
            .with_group_by(ident("name").dot_of("d"))
            .with_order(ident("name").dot_of("d"), Order::Asc),
        concat!(
            "SELECT `d`.`name` AS `department`, ",
            "JSON_ARRAYAGG(`u`.`name`) AS `user_names`, ",
            "JSON_OBJECTAGG(`u`.`name`, `u`.`salary`) AS `salary_map`",
            " FROM `departments` AS `d`",
            " INNER JOIN `users` AS `u` ON `u`.`department_id` = `d`.`id`",
            " WHERE `u`.`is_active` = b'1'",
            " GROUP BY `d`.`id`, `d`.`name`",
            " ORDER BY `d`.`name`",
        ),
    )
    .await;

    assert!(!rows.is_empty());
    // Departments should be alphabetically ordered
    for w in rows.windows(2) {
        assert!(w[0].department <= w[1].department);
    }
}

// -- -----------------------------------------------------------------------------
// -- 6. FULLTEXT SEARCH — MATCH ... AGAINST (MySQL-only syntax)
// -- Features: MATCH() AGAINST(), IN NATURAL LANGUAGE MODE, IN BOOLEAN MODE,
// --           relevance score
// -- Expected: products matching 'pro features' in name+description
// -- -----------------------------------------------------------------------------
// SELECT
//     p.id,
//     p.name,
//     p.description,
//     MATCH(p.name, p.description) AGAINST('pro features' IN NATURAL LANGUAGE MODE) AS relevance
// FROM products AS p
// WHERE MATCH(p.name, p.description) AGAINST('pro features' IN NATURAL LANGUAGE MODE)
// ORDER BY relevance DESC;

#[derive(Debug, Deserialize)]
struct FulltextResult {
    id: i64,
    name: String,
    description: Option<String>,
    relevance: f64,
}

#[tokio::test]
async fn test_my_q6_fulltext_search() {
    use vantage_sql::mysql::statements::primitives::FulltextMatch;

    let match_expr =
        FulltextMatch::new([ident("name").dot_of("p"), ident("description").dot_of("p")])
            .against("pro features")
            .natural_language_mode();

    let rows: Vec<FulltextResult> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("products", "p")
            .with_expression(ident("id").dot_of("p"), None)
            .with_expression(ident("name").dot_of("p"), None)
            .with_expression(ident("description").dot_of("p"), None)
            .with_expression(match_expr.clone(), Some("relevance".into()))
            .with_condition(match_expr)
            .with_order(ident("relevance"), Order::Desc),
        concat!(
            "SELECT `p`.`id`, `p`.`name`, `p`.`description`, ",
            "MATCH(`p`.`name`, `p`.`description`) AGAINST('pro features' IN NATURAL LANGUAGE MODE) AS `relevance`",
            " FROM `products` AS `p`",
            " WHERE MATCH(`p`.`name`, `p`.`description`) AGAINST('pro features' IN NATURAL LANGUAGE MODE)",
            " ORDER BY `relevance` DESC",
        ),
    )
    .await;

    assert!(!rows.is_empty());
    // Results should be ordered by relevance descending
    for w in rows.windows(2) {
        assert!(w[0].relevance >= w[1].relevance);
    }
}

// -- -----------------------------------------------------------------------------
// -- 7. REGEXP_LIKE / REGEXP — MySQL ICU regex (distinct syntax from PG ~)
// -- Features: REGEXP_LIKE(), REGEXP operator, REGEXP_SUBSTR()
// -- Expected: users whose email domain matches a pattern
// -- -----------------------------------------------------------------------------
// SELECT
//     u.id,
//     u.name,
//     u.email,
//     REGEXP_SUBSTR(u.email, '@(.+)$') AS domain_part,
//     REGEXP_LIKE(u.name, '^[A-E]') AS name_starts_a_to_e
// FROM users AS u
// WHERE u.email REGEXP '^[a-z]+@example\\.com$'
// ORDER BY u.name;

#[derive(Debug, Deserialize)]
struct UserRegexp {
    id: i64,
    name: String,
    email: String,
    domain_part: Option<String>,
    name_starts_a_to_e: i64,
}

#[tokio::test]
async fn test_my_q7_regexp() {
    let u_email = ident("email").dot_of("u");
    let u_name = ident("name").dot_of("u");

    let rows: Vec<UserRegexp> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("id").dot_of("u"), None)
            .with_expression(u_name.clone(), None)
            .with_expression(u_email.clone(), None)
            .with_expression(
                Fx::new("regexp_substr", [u_email.expr(), mysql_expr!("'@(.+)$'")]),
                Some("domain_part".into()),
            )
            .with_expression(
                Fx::new("regexp_like", [u_name.expr(), mysql_expr!("'^[A-E]'")]),
                Some("name_starts_a_to_e".into()),
            )
            .with_condition(u_email.regexp(mysql_expr!("{}", "^[a-z]+@example\\.com$")))
            .with_order(ident("name").dot_of("u"), Order::Asc),
        concat!(
            "SELECT `u`.`id`, `u`.`name`, `u`.`email`, ",
            "REGEXP_SUBSTR(`u`.`email`, '@(.+)$') AS `domain_part`, ",
            "REGEXP_LIKE(`u`.`name`, '^[A-E]') AS `name_starts_a_to_e`",
            " FROM `users` AS `u`",
            " WHERE `u`.`email` REGEXP '^[a-z]+@example\\.com$'",
            " ORDER BY `u`.`name`",
        ),
    )
    .await;

    assert!(!rows.is_empty());
    // All should match the email pattern
    for row in &rows {
        assert!(row.email.ends_with("@example.com"));
        assert!(row.domain_part.as_ref().unwrap().contains("example.com"));
    }
    // Alphabetically ordered by name
    for w in rows.windows(2) {
        assert!(w[0].name <= w[1].name);
    }
}

// -- -----------------------------------------------------------------------------
// -- 8. IF() / IFNULL — MySQL-specific conditional functions (not IIF, not COALESCE)
// -- Features: IF(cond, then, else), IFNULL(), NULLIF(), CONCAT_WS
// -- Expected: orders with status labels and null-safe notes
// -- -----------------------------------------------------------------------------
// SELECT
//     o.id,
//     o.total,
//     o.status,
//     IF(o.status = 'completed', 'Done', IF(o.status = 'cancelled', 'Void', 'Active')) AS status_label,
//     IFNULL(o.notes, '(no notes)') AS safe_notes,
//     NULLIF(o.status, 'pending') AS non_pending_status,
//     CONCAT_WS(' | ', o.status, o.notes) AS combined
// FROM orders AS o
// ORDER BY o.created_at DESC;

#[derive(Debug, Deserialize)]
struct OrderConditional {
    id: i64,
    total: f64,
    status: String,
    status_label: String,
    safe_notes: String,
    non_pending_status: Option<String>,
    combined: Option<String>,
}

#[tokio::test]
async fn test_my_q8_if_ifnull() {
    let o_status = ident("status").dot_of("o");
    let o_notes = ident("notes").dot_of("o");

    let rows: Vec<OrderConditional> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("orders", "o")
            .with_expression(ident("id").dot_of("o"), None)
            .with_expression(ident("total").dot_of("o"), None)
            .with_expression(o_status.clone(), None)
            .with_expression(
                Iif::new(
                    o_status.eq(mysql_expr!("{}", "completed")),
                    mysql_expr!("'Done'"),
                    Iif::new(
                        o_status.eq(mysql_expr!("{}", "cancelled")),
                        mysql_expr!("'Void'"),
                        mysql_expr!("'Active'"),
                    ),
                ),
                Some("status_label".into()),
            )
            .with_expression(
                Fx::new("ifnull", [o_notes.expr(), mysql_expr!("'(no notes)'")]),
                Some("safe_notes".into()),
            )
            .with_expression(
                Fx::new("nullif", [o_status.expr(), mysql_expr!("{}", "pending")]),
                Some("non_pending_status".into()),
            )
            .with_expression(
                Concat::new([o_status.expr(), o_notes.expr()])
                    .ws(mysql_expr!("' | '")),
                Some("combined".into()),
            )
            .with_order(ident("created_at").dot_of("o"), Order::Desc),
        concat!(
            "SELECT `o`.`id`, `o`.`total`, `o`.`status`, ",
            "IF(`o`.`status` = 'completed', 'Done', IF(`o`.`status` = 'cancelled', 'Void', 'Active')) AS `status_label`, ",
            "IFNULL(`o`.`notes`, '(no notes)') AS `safe_notes`, ",
            "NULLIF(`o`.`status`, 'pending') AS `non_pending_status`, ",
            "CONCAT_WS(' | ', `o`.`status`, `o`.`notes`) AS `combined`",
            " FROM `orders` AS `o`",
            " ORDER BY `o`.`created_at` DESC",
        ),
    )
    .await;

    assert_eq!(rows.len(), 15);
    // Check status_label mapping
    for row in &rows {
        match row.status.as_str() {
            "completed" => assert_eq!(row.status_label, "Done"),
            "cancelled" => assert_eq!(row.status_label, "Void"),
            _ => assert_eq!(row.status_label, "Active"),
        }
    }
    // pending orders should have non_pending_status = None
    let pending = rows.iter().find(|r| r.status == "pending").unwrap();
    assert!(pending.non_pending_status.is_none());
    // Orders without notes should show '(no notes)'
    assert!(rows.iter().any(|r| r.safe_notes == "(no notes)"));
}

// -- -----------------------------------------------------------------------------
// -- 9. GROUP BY ... WITH ROLLUP — MySQL-specific rollup syntax
// -- Features: WITH ROLLUP (MySQL syntax, not ROLLUP()), GROUPING()
// -- Expected: order revenue by status + month with sub-totals and grand total
// -- -----------------------------------------------------------------------------
// SELECT
//     IF(GROUPING(o.status), '** ALL **', o.status) AS status,
//     IF(GROUPING(DATE_FORMAT(o.created_at, '%Y-%m')), '** ALL **', DATE_FORMAT(o.created_at, '%Y-%m')) AS month,
//     COUNT(*) AS order_count,
//     SUM(o.total) AS revenue
// FROM orders AS o
// WHERE o.status != 'cancelled'
// GROUP BY o.status, DATE_FORMAT(o.created_at, '%Y-%m') WITH ROLLUP
// ORDER BY GROUPING(o.status), o.status, month;

#[derive(Debug, Deserialize)]
struct RollupRow {
    status: Option<String>,
    month: Option<String>,
    order_count: i64,
    revenue: f64,
    is_status_total: i64,
    is_month_total: i64,
}

#[tokio::test]
async fn test_my_q9_rollup() {
    let o_status = ident("status").dot_of("o");
    let date_fmt = Fx::new(
        "date_format",
        [
            ident("created_at").dot_of("o").expr(),
            mysql_expr!("'%Y-%m'"),
        ],
    );
    let grouping_status = Fx::new("grouping", [o_status.expr()]);
    let grouping_month = Fx::new("grouping", [date_fmt.expr()]);

    let rows: Vec<RollupRow> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("orders", "o")
            .with_expression(o_status.clone(), None)
            .with_expression(date_fmt.clone(), Some("month".into()))
            .with_expression(
                Fx::new("count", [mysql_expr!("*")]),
                Some("order_count".into()),
            )
            .with_expression(
                Fx::new("sum", [ident("total").dot_of("o").expr()]),
                Some("revenue".into()),
            )
            .with_expression(grouping_status.clone(), Some("is_status_total".into()))
            .with_expression(grouping_month.clone(), Some("is_month_total".into()))
            .with_condition(o_status.ne(mysql_expr!("{}", "cancelled")))
            .with_group_by(o_status.clone())
            .with_group_by(ident("month"))
            .with_rollup()
            .with_order(grouping_status, Order::Asc)
            .with_order(o_status, Order::Asc)
            .with_order(ident("month"), Order::Asc),
        concat!(
            "SELECT `o`.`status`, ",
            "DATE_FORMAT(`o`.`created_at`, '%Y-%m') AS `month`, ",
            "COUNT(*) AS `order_count`, ",
            "SUM(`o`.`total`) AS `revenue`, ",
            "GROUPING(`o`.`status`) AS `is_status_total`, ",
            "GROUPING(DATE_FORMAT(`o`.`created_at`, '%Y-%m')) AS `is_month_total`",
            " FROM `orders` AS `o`",
            " WHERE `o`.`status` != 'cancelled'",
            " GROUP BY `o`.`status`, `month` WITH ROLLUP",
            " ORDER BY GROUPING(`o`.`status`), `o`.`status`, `month`",
        ),
    )
    .await;

    // Should have detail rows + sub-totals + grand total
    assert!(rows.len() > 3);
    // Last row is the grand total
    let last = rows.last().unwrap();
    assert_eq!(last.is_status_total, 1);
    assert_eq!(last.is_month_total, 1);
    // There should be detail rows with both grouping flags as 0
    let detail = rows
        .iter()
        .find(|r| r.is_status_total == 0 && r.is_month_total == 0)
        .unwrap();
    assert!(detail.status.is_some());
    assert!(detail.month.is_some());
}

// -- -----------------------------------------------------------------------------
// -- 10. BIT operations — MySQL BIT type arithmetic + BIN() display
// -- Features: BIT type, & (bitwise AND), | (bitwise OR), BIN(), BIT_COUNT()
// -- Expected: schedules with decoded bitmask flags
// -- -----------------------------------------------------------------------------
// SELECT
//     u.name,
//     s.day_of_week,
//     s.start_time,
//     s.end_time,
//     BIN(s.flags) AS flags_binary,
//     IF(s.flags & b'00000001', 'Yes', 'No') AS is_remote,
//     IF(s.flags & b'00000010', 'Yes', 'No') AS is_flexible,
//     IF(s.flags & b'00000100', 'Yes', 'No') AS is_oncall,
//     BIT_COUNT(s.flags) AS active_flag_count,
//     TIMEDIFF(s.end_time, s.start_time) AS duration
// FROM schedules AS s
// INNER JOIN users AS u ON u.id = s.user_id
// ORDER BY u.name, s.day_of_week;

#[derive(Debug, Deserialize)]
struct ScheduleBit {
    name: String,
    day_of_week: i64,
    start_time: String,
    end_time: String,
    flags_binary: String,
    is_remote: String,
    is_flexible: String,
    is_oncall: String,
    active_flag_count: i64,
    duration: String,
}

#[tokio::test]
async fn test_my_q10_bit_operations() {
    let flags = ident("flags").dot_of("s");

    let rows: Vec<ScheduleBit> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("schedules", "s")
            .with_join(MysqlSelectJoin::inner(
                "users",
                "u",
                ident("id").dot_of("u").eq(ident("user_id").dot_of("s")),
            ))
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("day_of_week").dot_of("s"), None)
            .with_expression(ident("start_time").dot_of("s"), None)
            .with_expression(ident("end_time").dot_of("s"), None)
            .with_expression(Fx::new("bin", [flags.expr()]), Some("flags_binary".into()))
            .with_expression(
                Iif::new(
                    flags.bitand(mysql_expr!("b'00000001'")),
                    mysql_expr!("'Yes'"),
                    mysql_expr!("'No'"),
                ),
                Some("is_remote".into()),
            )
            .with_expression(
                Iif::new(
                    flags.bitand(mysql_expr!("b'00000010'")),
                    mysql_expr!("'Yes'"),
                    mysql_expr!("'No'"),
                ),
                Some("is_flexible".into()),
            )
            .with_expression(
                Iif::new(
                    flags.bitand(mysql_expr!("b'00000100'")),
                    mysql_expr!("'Yes'"),
                    mysql_expr!("'No'"),
                ),
                Some("is_oncall".into()),
            )
            .with_expression(
                Fx::new("bit_count", [flags.expr()]),
                Some("active_flag_count".into()),
            )
            .with_expression(
                Fx::new(
                    "timediff",
                    [
                        ident("end_time").dot_of("s").expr(),
                        ident("start_time").dot_of("s").expr(),
                    ],
                ),
                Some("duration".into()),
            )
            .with_order(ident("name").dot_of("u"), Order::Asc)
            .with_order(ident("day_of_week").dot_of("s"), Order::Asc),
        concat!(
            "SELECT `u`.`name`, `s`.`day_of_week`, `s`.`start_time`, `s`.`end_time`, ",
            "BIN(`s`.`flags`) AS `flags_binary`, ",
            "IF(`s`.`flags` & b'00000001', 'Yes', 'No') AS `is_remote`, ",
            "IF(`s`.`flags` & b'00000010', 'Yes', 'No') AS `is_flexible`, ",
            "IF(`s`.`flags` & b'00000100', 'Yes', 'No') AS `is_oncall`, ",
            "BIT_COUNT(`s`.`flags`) AS `active_flag_count`, ",
            "TIMEDIFF(`s`.`end_time`, `s`.`start_time`) AS `duration`",
            " FROM `schedules` AS `s`",
            " INNER JOIN `users` AS `u` ON `u`.`id` = `s`.`user_id`",
            " ORDER BY `u`.`name`, `s`.`day_of_week`",
        ),
    )
    .await;

    assert!(!rows.is_empty());
    // Alice Mon: remote+flexible (flags=0b11) => is_remote=Yes, is_flexible=Yes, is_oncall=No
    let alice_mon = rows
        .iter()
        .find(|r| r.name == "Alice Chen" && r.day_of_week == 1)
        .unwrap();
    assert_eq!(alice_mon.is_remote, "Yes");
    assert_eq!(alice_mon.is_flexible, "Yes");
    assert_eq!(alice_mon.is_oncall, "No");
    assert_eq!(alice_mon.active_flag_count, 2);
}

// -- -----------------------------------------------------------------------------
// -- 11. YEAR type + date functions — MySQL-only YEAR column, STR_TO_DATE,
// --     DATE_FORMAT, LAST_DAY, DATEDIFF
// -- Features: YEAR type in WHERE, DATE_FORMAT(), LAST_DAY(), DATEDIFF(),
// --           STR_TO_DATE(), TIMESTAMPDIFF()
// -- Expected: users hired after 2022 with account age info
// -- -----------------------------------------------------------------------------
// SELECT
//     u.name,
//     u.hire_year,
//     u.created_at,
//     DATE_FORMAT(u.created_at, '%W, %M %D %Y') AS formatted_date,
//     LAST_DAY(u.created_at) AS month_end,
//     DATEDIFF(NOW(), u.created_at) AS days_since_creation,
//     TIMESTAMPDIFF(MONTH, u.created_at, NOW()) AS months_active
// FROM users AS u
// WHERE u.hire_year >= 2023
// ORDER BY u.hire_year, u.created_at;

#[derive(Debug, Deserialize)]
struct UserDateInfo {
    name: String,
    hire_year: i64,
    created_at: String,
    formatted_date: String,
    month_end: String,
    days_since_creation: i64,
    months_active: i64,
}

#[tokio::test]
async fn test_my_q11_year_date_functions() {
    use vantage_sql::primitives::date_format::DateFormat;

    let created_at = ident("created_at").dot_of("u");
    let now = Fx::new("now", Vec::<Expression<_>>::new());

    let rows: Vec<UserDateInfo> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("hire_year").dot_of("u"), None)
            .with_expression(created_at.clone(), None)
            .with_expression(
                DateFormat::raw_format(created_at.clone(), "%W, %M %D %Y"),
                Some("formatted_date".into()),
            )
            .with_expression(
                Fx::new("last_day", [created_at.expr()]),
                Some("month_end".into()),
            )
            .with_expression(
                Fx::new("datediff", [now.expr(), created_at.expr()]),
                Some("days_since_creation".into()),
            )
            .with_expression(
                Fx::new(
                    "timestampdiff",
                    [mysql_expr!("MONTH"), created_at.expr(), now.expr()],
                ),
                Some("months_active".into()),
            )
            .with_condition(
                ident("hire_year")
                    .dot_of("u")
                    .gte(mysql_expr!("{}", 2023i64)),
            )
            .with_order(ident("hire_year").dot_of("u"), Order::Asc)
            .with_order(created_at, Order::Asc),
        concat!(
            "SELECT `u`.`name`, `u`.`hire_year`, `u`.`created_at`, ",
            "DATE_FORMAT(`u`.`created_at`, '%W, %M %D %Y') AS `formatted_date`, ",
            "LAST_DAY(`u`.`created_at`) AS `month_end`, ",
            "DATEDIFF(NOW(), `u`.`created_at`) AS `days_since_creation`, ",
            "TIMESTAMPDIFF(MONTH, `u`.`created_at`, NOW()) AS `months_active`",
            " FROM `users` AS `u`",
            " WHERE `u`.`hire_year` >= 2023",
            " ORDER BY `u`.`hire_year`, `u`.`created_at`",
        ),
    )
    .await;

    // Users hired 2023+: Carol, Dan, Frank, Grace, Hank, Iris, Jake, Karen
    assert!(rows.len() >= 7);
    // All should have hire_year >= 2023
    for row in &rows {
        assert!(row.hire_year >= 2023);
        assert!(row.days_since_creation > 0);
        assert!(row.months_active > 0);
        assert!(!row.formatted_date.is_empty());
    }
}

// -- -----------------------------------------------------------------------------
// -- 12. SPATIAL — ST_Distance_Sphere, ST_X/ST_Y (MySQL spatial functions)
// -- Features: ST_Distance_Sphere(), ST_X(), ST_Y(), SRID, spatial query
// -- Expected: products near London (51.5, -0.12) within 1000km
// -- Note: location is NOT NULL with default POINT(0 0), so we filter out dummy points
// -- -----------------------------------------------------------------------------
// SELECT
//     p.id,
//     p.name,
//     ST_X(p.location) AS latitude,
//     ST_Y(p.location) AS longitude,
//     ROUND(ST_Distance_Sphere(p.location, ST_GeomFromText('POINT(51.5 -0.12)', 0)) / 1000, 1) AS distance_km
// FROM products AS p
// WHERE ST_X(p.location) != 0
// ORDER BY distance_km ASC;

#[derive(Debug, Deserialize)]
struct ProductSpatial {
    id: i64,
    name: String,
    lon: f64,
    lat: f64,
    distance_km: f64,
}

#[tokio::test]
async fn test_my_q12_spatial() {
    use vantage_sql::primitives::point::Point;

    let location = ident("location").dot_of("p");
    let london = Point::new(-0.12, 51.5);
    let distance = mysql_expr!(
        "{} / 1000",
        (Fx::new("st_distance_sphere", [location.expr(), london.expr()]))
    );

    let rows: Vec<ProductSpatial> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("products", "p")
            .with_expression(ident("id").dot_of("p"), None)
            .with_expression(ident("name").dot_of("p"), None)
            .with_expression(
                Fx::new("st_x", [location.expr()]),
                Some("lon".into()),
            )
            .with_expression(
                Fx::new("st_y", [location.expr()]),
                Some("lat".into()),
            )
            .with_expression(
                Fx::new("round", [distance, mysql_expr!("1")]),
                Some("distance_km".into()),
            )
            .with_condition(
                Fx::new("st_x", [location.expr()]).ne(mysql_expr!("{}", 0i64)),
            )
            .with_order(ident("distance_km"), Order::Asc),
        concat!(
            "SELECT `p`.`id`, `p`.`name`, ",
            "ST_X(`p`.`location`) AS `lon`, ",
            "ST_Y(`p`.`location`) AS `lat`, ",
            "ROUND(ST_DISTANCE_SPHERE(`p`.`location`, ST_GeomFromText('POINT(-0.12 51.5)', 0)) / 1000, 1) AS `distance_km`",
            " FROM `products` AS `p`",
            " WHERE ST_X(`p`.`location`) != 0",
            " ORDER BY `distance_km`",
        ),
    )
    .await;

    // Products with real locations: Widget Pro (London area), Widget Basic (Berlin),
    // Gadget Pro Max (Paris), USB-C Cable (NYC), Monitor (Tokyo), Keyboard (SF)
    assert_eq!(rows.len(), 6);
    // Closest to London should be Widget Pro (near London)
    assert_eq!(rows[0].name, "Widget Pro");
    // Distances should be ascending
    for w in rows.windows(2) {
        assert!(w[0].distance_km <= w[1].distance_km);
    }
}

// -- -----------------------------------------------------------------------------
// -- 13. Generated columns (VIRTUAL + STORED), reading computed values
// -- Features: select from VIRTUAL and STORED generated columns, use in WHERE
// -- Expected: users with their display_name (virtual) and salary_band (stored)
// -- -----------------------------------------------------------------------------
// SELECT
//     u.id,
//     u.display_name,
//     u.salary_band,
//     u.salary,
//     u.role
// FROM users AS u
// WHERE u.salary_band IN ('senior', 'mid')
// ORDER BY u.salary DESC;

#[derive(Debug, Deserialize)]
struct UserGenerated {
    id: i64,
    display_name: String,
    salary_band: String,
    salary: f64,
    role: String,
}

#[tokio::test]
async fn test_my_q13_generated_columns() {
    let rows: Vec<UserGenerated> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("id").dot_of("u"), None)
            .with_expression(ident("display_name").dot_of("u"), None)
            .with_expression(ident("salary_band").dot_of("u"), None)
            .with_expression(ident("salary").dot_of("u"), None)
            .with_expression(ident("role").dot_of("u"), None)
            .with_condition(ident("salary_band").dot_of("u").in_list(&["senior", "mid"]))
            .with_order(ident("salary").dot_of("u"), Order::Desc),
        concat!(
            "SELECT `u`.`id`, `u`.`display_name`, `u`.`salary_band`, `u`.`salary`, `u`.`role`",
            " FROM `users` AS `u`",
            " WHERE `u`.`salary_band` IN ('senior', 'mid')",
            " ORDER BY `u`.`salary` DESC",
        ),
    )
    .await;

    assert!(!rows.is_empty());
    // All should be senior or mid
    for row in &rows {
        assert!(
            row.salary_band == "senior" || row.salary_band == "mid",
            "unexpected band: {}",
            row.salary_band
        );
        // display_name is VIRTUAL: "Name <email>"
        assert!(row.display_name.contains('<'));
    }
    // Ordered by salary DESC
    for w in rows.windows(2) {
        assert!(w[0].salary >= w[1].salary);
    }
}

// -- -----------------------------------------------------------------------------
// -- 14. HEX / UNHEX / UUID to BINARY — MySQL binary UUID pattern
// -- Features: HEX(), UNHEX(), INSERT() for UUID formatting, BINARY(16) column
// -- Expected: audit log entries with session UUID decoded from BINARY(16)
// -- -----------------------------------------------------------------------------
// SELECT
//     a.id,
//     a.table_name,
//     a.action,
//     a.details,
//     HEX(a.session_id) AS session_hex,
//     CASE WHEN a.session_id IS NOT NULL THEN
//         INSERT(INSERT(INSERT(INSERT(
//             HEX(a.session_id),
//         9, 0, '-'), 14, 0, '-'), 19, 0, '-'), 24, 0, '-')
//     END AS session_uuid,
//     a.changed_at
// FROM audit_log AS a
// ORDER BY a.changed_at DESC;

#[derive(Debug, Deserialize)]
struct AuditEntry {
    id: i64,
    table_name: String,
    action: String,
    details: Option<String>,
    session_hex: Option<String>,
    session_uuid: Option<String>,
    changed_at: String,
}

#[tokio::test]
async fn test_my_q14_hex_uuid() {
    use vantage_sql::primitives::case::Case;

    let session_id = ident("session_id").dot_of("a");
    let hex_session = Fx::new("hex", [session_id.expr()]);

    // INSERT(str, pos, 0, '-') — inserts a dash at `pos` without removing chars
    let my_insert = |expr: Expression<_>, pos: i64| -> Expression<_> {
        Fx::new(
            "insert",
            [
                expr,
                mysql_expr!("{}", pos),
                mysql_expr!("{}", 0i64),
                mysql_expr!("'-'"),
            ],
        )
        .expr()
    };

    let uuid_formatted = my_insert(
        my_insert(my_insert(my_insert(hex_session.expr(), 9), 14), 19),
        24,
    );

    let rows: Vec<AuditEntry> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("audit_log", "a")
            .with_expression(ident("id").dot_of("a"), None)
            .with_expression(ident("table_name").dot_of("a"), None)
            .with_expression(ident("action").dot_of("a"), None)
            .with_expression(ident("details").dot_of("a"), None)
            .with_expression(hex_session, Some("session_hex".into()))
            .with_expression(
                Case::new().when(
                    mysql_expr!("{} IS NOT NULL", (session_id)),
                    uuid_formatted,
                ),
                Some("session_uuid".into()),
            )
            .with_expression(ident("changed_at").dot_of("a"), None)
            .with_order(ident("changed_at").dot_of("a"), Order::Desc),
        concat!(
            "SELECT `a`.`id`, `a`.`table_name`, `a`.`action`, `a`.`details`, ",
            "HEX(`a`.`session_id`) AS `session_hex`, ",
            "CASE WHEN `a`.`session_id` IS NOT NULL ",
            "THEN INSERT(INSERT(INSERT(INSERT(HEX(`a`.`session_id`), 9, 0, '-'), 14, 0, '-'), 19, 0, '-'), 24, 0, '-') ",
            "END AS `session_uuid`",
            ", `a`.`changed_at`",
            " FROM `audit_log` AS `a`",
            " ORDER BY `a`.`changed_at` DESC",
        ),
    )
    .await;

    assert_eq!(rows.len(), 6);
    // First two audit entries have session_id set
    let with_session: Vec<_> = rows.iter().filter(|r| r.session_uuid.is_some()).collect();
    assert_eq!(with_session.len(), 2);
    // UUID format: 8-4-4-4-12
    for row in &with_session {
        let uuid = row.session_uuid.as_ref().unwrap();
        let parts: Vec<_> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5, "UUID should have 5 parts: {uuid}");
    }
}

// -- -----------------------------------------------------------------------------
// -- 15. Window functions + MySQL-specific aggregates in same query
// -- Features: ROW_NUMBER, DENSE_RANK, NTILE, named WINDOW,
// --           GROUP_CONCAT OVER (window), PERCENT_RANK
// -- Expected: salary ranking within department with grouping
// -- -----------------------------------------------------------------------------
// SELECT
//     u.name,
//     d.name AS department,
//     u.salary,
//     ROW_NUMBER() OVER dept_sal AS row_num,
//     DENSE_RANK() OVER dept_sal AS salary_rank,
//     NTILE(3) OVER dept_sal AS salary_tercile,
//     PERCENT_RANK() OVER dept_sal AS pct_rank,
//     SUM(u.salary) OVER (
//         PARTITION BY u.department_id
//         ORDER BY u.salary DESC
//         ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
//     ) AS running_total,
//     FIRST_VALUE(u.name) OVER dept_sal AS top_earner
// FROM users AS u
// INNER JOIN departments AS d ON d.id = u.department_id
// WHERE u.is_active = b'1'
// WINDOW dept_sal AS (PARTITION BY u.department_id ORDER BY u.salary DESC)
// ORDER BY d.name, u.salary DESC;

#[derive(Debug, Deserialize)]
struct SalaryRanking {
    name: String,
    department: String,
    salary: f64,
    row_num: i64,
    salary_rank: i64,
    salary_tercile: i64,
    pct_rank: f64,
    running_total: f64,
    top_earner: String,
}

#[tokio::test]
async fn test_my_q15_window_functions() {
    use vantage_sql::primitives::select::window::Window;

    let salary = ident("salary").dot_of("u");
    let dept_id = ident("department_id").dot_of("u");
    let named = Window::named("dept_sal");

    let rows: Vec<SalaryRanking> = check_and_run(
        &MysqlSelect::new()
            .with_source_as("users", "u")
            .with_join(MysqlSelectJoin::inner(
                "departments",
                "d",
                ident("id").dot_of("d").eq(dept_id.clone()),
            ))
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("name").dot_of("d"), Some("department".into()))
            .with_expression(salary.clone(), None)
            .with_expression(
                named.apply(Fx::new("row_number", Vec::<Expression<_>>::new())),
                Some("row_num".into()),
            )
            .with_expression(
                named.apply(Fx::new("dense_rank", Vec::<Expression<_>>::new())),
                Some("salary_rank".into()),
            )
            .with_expression(
                named.apply(Fx::new("ntile", [mysql_expr!("{}", 3i64)])),
                Some("salary_tercile".into()),
            )
            .with_expression(
                named.apply(Fx::new("percent_rank", Vec::<Expression<_>>::new())),
                Some("pct_rank".into()),
            )
            .with_expression(
                Window::new()
                    .partition_by(dept_id.clone())
                    .order_by(salary.clone(), Order::Desc)
                    .rows("UNBOUNDED PRECEDING", "CURRENT ROW")
                    .apply(Fx::new("sum", [salary.expr()])),
                Some("running_total".into()),
            )
            .with_expression(
                named.apply(Fx::new("first_value", [ident("name").dot_of("u").expr()])),
                Some("top_earner".into()),
            )
            .with_condition(mysql_expr!("{} = b'1'", (ident("is_active").dot_of("u"))))
            .with_window(
                "dept_sal",
                Window::new()
                    .partition_by(dept_id)
                    .order_by(salary, Order::Desc),
            )
            .with_order(ident("name").dot_of("d"), Order::Asc)
            .with_order(ident("salary").dot_of("u"), Order::Desc),
        concat!(
            "SELECT `u`.`name`, `d`.`name` AS `department`, `u`.`salary`, ",
            "ROW_NUMBER() OVER dept_sal AS `row_num`, ",
            "DENSE_RANK() OVER dept_sal AS `salary_rank`, ",
            "NTILE(3) OVER dept_sal AS `salary_tercile`, ",
            "PERCENT_RANK() OVER dept_sal AS `pct_rank`, ",
            "SUM(`u`.`salary`) OVER (PARTITION BY `u`.`department_id` ORDER BY `u`.`salary` DESC ",
            "ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS `running_total`, ",
            "FIRST_VALUE(`u`.`name`) OVER dept_sal AS `top_earner`",
            " FROM `users` AS `u`",
            " INNER JOIN `departments` AS `d` ON `d`.`id` = `u`.`department_id`",
            " WHERE `u`.`is_active` = b'1'",
            " WINDOW dept_sal AS (PARTITION BY `u`.`department_id` ORDER BY `u`.`salary` DESC)",
            " ORDER BY `d`.`name`, `u`.`salary` DESC",
        ),
    )
    .await;

    assert!(!rows.is_empty());
    // First row in each department should have row_num=1 and salary_rank=1
    let first_backend = rows.iter().find(|r| r.department == "Backend").unwrap();
    assert_eq!(first_backend.row_num, 1);
    assert_eq!(first_backend.salary_rank, 1);
    // Top earner should be the same for all rows in a department
    let backend_rows: Vec<_> = rows.iter().filter(|r| r.department == "Backend").collect();
    let top = &backend_rows[0].name;
    for row in &backend_rows {
        assert_eq!(&row.top_earner, top);
    }
    // Running total should increase (or stay same) within each department
    for row in &backend_rows {
        assert!(row.running_total >= row.salary);
    }
}
