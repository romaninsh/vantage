//! PostgreSQL-specific query builder tests.
//! These exercise features that only exist in PostgreSQL (DISTINCT ON, array ops, etc.).
//! Runs against the vantage_pg database (v4_pg.sql schema).

#![allow(dead_code)]

use serde::Deserialize;
use vantage_expressions::{ExprDataSource, Expressive, Order, Selectable};
use vantage_sql::postgres::PostgresDB;
use vantage_sql::postgres::statements::PostgresSelect;
use vantage_sql::postgres::statements::select::join::PostgresSelectJoin;
use vantage_sql::postgres_expr;
use vantage_sql::primitives::identifier::ident;
use vantage_table::operation::Operation;
use vantage_types::{Record, TryFromRecord};

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage_pg";

async fn get_db() -> PostgresDB {
    PostgresDB::connect(PG_URL)
        .await
        .expect("Failed to connect to vantage_pg")
}

/// Checks that `select.preview()` matches `expected_sql`, then executes the
/// query and returns deserialized rows.
async fn check_and_run<T: for<'de> Deserialize<'de>>(
    select: &PostgresSelect,
    expected_sql: &str,
) -> Vec<T> {
    assert_eq!(select.preview(), expected_sql);

    let db = get_db().await;
    let result = db.execute(&select.expr()).await.unwrap();
    let rows = result.into_value();
    let arr = rows.as_array().unwrap();

    let records: Vec<Record<serde_json::Value>> = arr.iter().map(|v| v.clone().into()).collect();
    records
        .into_iter()
        .map(|r| T::from_record(r).unwrap())
        .collect()
}

// -- ---------------------------------------------------------------------------
// -- 1. DISTINCT ON — return one row per group (PG-only)
// -- Features: DISTINCT ON (expr), ORDER BY must start with the DISTINCT exprs
// -- Expected: most recent order per user
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LatestOrder {
    user_id: String,
    name: String,
    order_id: i64,
    total: f64,
    status: String,
}

#[tokio::test]
async fn test_pg_q1_distinct_on() {
    let rows: Vec<LatestOrder> = check_and_run(
        &PostgresSelect::new()
            .with_distinct_on(ident("user_id").dot_of("o"))
            .with_source_as("orders", "o")
            .with_expression(ident("user_id").dot_of("o"), None)
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("id").dot_of("o").with_alias("order_id"), None)
            .with_expression(ident("total").dot_of("o"), None)
            .with_expression(ident("status").dot_of("o"), None)
            .with_join(PostgresSelectJoin::inner(
                "users",
                "u",
                ident("id").dot_of("u").eq(ident("user_id").dot_of("o")),
            ))
            .with_order(ident("user_id").dot_of("o"), Order::Asc)
            .with_order(ident("created_at").dot_of("o"), Order::Desc),
        concat!(
            r#"SELECT DISTINCT ON ("o"."user_id") "#,
            r#""o"."user_id", "u"."name", "o"."id" AS "order_id", "#,
            r#""o"."total", "o"."status" "#,
            r#"FROM "orders" AS "o" "#,
            r#"INNER JOIN "users" AS "u" ON "u"."id" = "o"."user_id" "#,
            r#"ORDER BY "o"."user_id", "o"."created_at" DESC"#,
        ),
    )
    .await;

    // 8 users have orders, one row each (most recent)
    assert_eq!(rows.len(), 8);

    // Alice's most recent order is #15 (pending, $50)
    let alice = rows.iter().find(|r| r.name == "Alice Chen").unwrap();
    assert_eq!(alice.order_id, 15);
    assert_eq!(alice.status, "pending");

    // Leo's most recent order is #14 (completed, $275)
    let leo = rows.iter().find(|r| r.name == "Leo Russo").unwrap();
    assert_eq!(leo.order_id, 14);
    assert_eq!(leo.total, 275.0);
}

// -- ---------------------------------------------------------------------------
// -- 2. Array operators — ANY, @>, array_length
// -- Features: @> (array contains), = ANY(array), array_length(), NULLS LAST
// -- Expected: users who have 'rust' or 'python' in their skills
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UserSkills {
    id: String,
    name: String,
    skills: Vec<String>,
    skill_count: Option<i64>,
}

#[tokio::test]
async fn test_pg_q2_array_ops() {
    use vantage_sql::primitives::fx::Fx;

    let skills = ident("skills").dot_of("u");

    let rows: Vec<UserSkills> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("id").dot_of("u"), None)
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(skills.clone(), None)
            .with_expression(
                Fx::new("array_length", [skills.expr(), postgres_expr!("{}", 1i32)]),
                Some("skill_count".into()),
            )
            .with_condition(postgres_expr!(
                "{} @> ARRAY[{}] OR {} = ANY({})",
                (skills.clone()),
                "rust",
                "python",
                (skills)
            ))
            .with_order(ident("skill_count"), Order::Desc.nulls_last()),
        concat!(
            r#"SELECT "u"."id", "u"."name", "u"."skills", "#,
            r#"ARRAY_LENGTH("u"."skills", 1) AS "skill_count" "#,
            r#"FROM "users" AS "u" "#,
            r#"WHERE "u"."skills" @> ARRAY['rust'] OR 'python' = ANY("u"."skills") "#,
            r#"ORDER BY "skill_count" DESC NULLS LAST"#,
        ),
    )
    .await;

    // Alice (rust), Bob (python), Grace (python)
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, "Alice Chen");
    assert_eq!(rows[0].skill_count, Some(3));
    assert!(rows[0].skills.contains(&"rust".to_string()));

    // Bob and Grace both have 2 skills
    assert_eq!(rows[1].skill_count, Some(2));
    assert_eq!(rows[2].skill_count, Some(2));
}

// -- ---------------------------------------------------------------------------
// -- 3. JSONB operators — ->, ->>, @>, ?, #>, nested path, ::NUMERIC cast
// -- Features: -> (json object), ->> (text), @> containment,
// --           jsonb_exists (? operator), #> path, cast
// -- Expected: black electronics with specs.voltage, rating >= 4.0
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ProductJsonb {
    id: i64,
    name: String,
    color: String,
    rating: f64,
    voltage: Option<String>,
    watts_json: Option<serde_json::Value>,
}

#[tokio::test]
async fn test_pg_q3_jsonb_ops() {
    use vantage_expressions::expr_any;
    use vantage_sql::primitives::fx::Fx;
    use vantage_sql::primitives::json_extract::JsonExtract;

    let metadata = ident("metadata").dot_of("p");
    let rating = JsonExtract::new(metadata.clone(), "rating").cast("NUMERIC");

    // Inline SQL literal — not a bind parameter
    let lit = |s: &str| -> vantage_expressions::Expression<_> { expr_any!(format!("'{s}'")) };

    let rows: Vec<ProductJsonb> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("products", "p")
            .with_expression(ident("id").dot_of("p"), None)
            .with_expression(ident("name").dot_of("p"), None)
            .with_expression(
                JsonExtract::new(metadata.clone(), "color").with_alias("color"),
                None,
            )
            .with_expression(rating.clone(), Some("rating".into()))
            .with_expression(
                JsonExtract::new(metadata.clone(), ["specs", "voltage"]).with_alias("voltage"),
                None,
            )
            .with_expression(
                postgres_expr!("{} #> {}", (metadata.clone()), (lit("{specs,watts}"))),
                Some("watts_json".into()),
            )
            .with_condition(postgres_expr!(
                "{} @> {}",
                (metadata.clone()),
                (lit(r#"{"color": "black"}"#))
            ))
            .with_condition(Fx::new(
                "jsonb_exists",
                [metadata.expr(), postgres_expr!("{}", "rating")],
            ))
            .with_condition(rating.gte(4.0f64))
            .with_order(ident("rating"), Order::Desc),
        concat!(
            r#"SELECT "p"."id", "p"."name", "#,
            r#""p"."metadata" ->> 'color' AS "color", "#,
            r#"CAST("p"."metadata" ->> 'rating' AS NUMERIC) AS "rating", "#,
            r#""p"."metadata" -> 'specs' ->> 'voltage' AS "voltage", "#,
            r#""p"."metadata" #> '{specs,watts}' AS "watts_json" "#,
            r#"FROM "products" AS "p" "#,
            r#"WHERE "p"."metadata" @> '{"color": "black"}' "#,
            r#"AND JSONB_EXISTS("p"."metadata", 'rating') "#,
            r#"AND CAST("p"."metadata" ->> 'rating' AS NUMERIC) >= 4.0 "#,
            r#"ORDER BY "rating" DESC"#,
        ),
    )
    .await;

    // Widget Pro (4.7), Monitor 27" (4.6), USB-C Cable (4.0) — all black, rating >= 4.0
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].name, "Widget Pro");
    assert_eq!(rows[0].color, "black");
    assert_eq!(rows[0].voltage, Some("5".to_string()));

    // USB-C Cable has no specs
    assert_eq!(rows[2].name, "USB-C Cable");
    assert_eq!(rows[2].voltage, None);
    assert_eq!(rows[2].watts_json, None);
}

// -- ---------------------------------------------------------------------------
// -- 4. LATERAL JOIN — correlated subquery in FROM
// -- Features: LEFT JOIN LATERAL, ON TRUE, LIMIT inside lateral
// -- Expected: each user with their 2 most recent orders
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UserRecentOrder {
    name: String,
    order_id: Option<i64>,
    total: Option<f64>,
    status: Option<String>,
}

#[tokio::test]
async fn test_pg_q4_lateral_join() {
    let lateral_subquery = PostgresSelect::new()
        .with_source_as("orders", "o")
        .with_expression(ident("id").dot_of("o").with_alias("order_id"), None)
        .with_expression(ident("total").dot_of("o"), None)
        .with_expression(ident("status").dot_of("o"), None)
        .with_expression(ident("created_at").dot_of("o"), None)
        .with_condition(ident("user_id").dot_of("o").eq(ident("id").dot_of("u")))
        .with_order(ident("created_at").dot_of("o"), Order::Desc)
        .with_limit(Some(2), None);

    let rows: Vec<UserRecentOrder> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("order_id").dot_of("recent"), None)
            .with_expression(ident("total").dot_of("recent"), None)
            .with_expression(ident("status").dot_of("recent"), None)
            .with_join(PostgresSelectJoin::left_lateral(lateral_subquery, "recent"))
            .with_order(ident("name").dot_of("u"), Order::Asc)
            .with_order(ident("created_at").dot_of("recent"), Order::Desc.nulls_last()),
        concat!(
            r#"SELECT "u"."name", "recent"."order_id", "recent"."total", "recent"."status" "#,
            r#"FROM "users" AS "u" "#,
            r#"LEFT JOIN LATERAL (SELECT "o"."id" AS "order_id", "o"."total", "o"."status", "o"."created_at" "#,
            r#"FROM "orders" AS "o" "#,
            r#"WHERE "o"."user_id" = "u"."id" "#,
            r#"ORDER BY "o"."created_at" DESC "#,
            r#"LIMIT 2) AS "recent" ON TRUE "#,
            r#"ORDER BY "u"."name", "recent"."created_at" DESC NULLS LAST"#,
        ),
    )
    .await;

    // 12 users, 8 have orders (some have 2), 4 have none → 17 rows
    assert_eq!(rows.len(), 17);

    // Alice has 2 most recent orders: #15 (pending) and #3 (shipped)
    let alice_rows: Vec<&UserRecentOrder> =
        rows.iter().filter(|r| r.name == "Alice Chen").collect();
    assert_eq!(alice_rows.len(), 2);
    assert_eq!(alice_rows[0].order_id, Some(15));
    assert_eq!(alice_rows[0].status, Some("pending".to_string()));

    // Dan Brown has no orders → null fields
    let dan = rows.iter().find(|r| r.name == "Dan Brown").unwrap();
    assert_eq!(dan.order_id, None);
}

// -- ---------------------------------------------------------------------------
// -- 5. generate_series as table source — synthetic date grid
// -- Features: generate_series(), ::DATE cast, LEFT JOIN on cast, COALESCE
// -- Expected: daily order counts for April 1-10, including zero-days
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DailyRevenue {
    day: String,
    order_count: i64,
    daily_revenue: f64,
}

#[tokio::test]
async fn test_pg_q5_generate_series() {
    use vantage_sql::primitives::fx::Fx;

    let series = Fx::new(
        "generate_series",
        [
            postgres_expr!("{}::DATE", "2025-04-01"),
            postgres_expr!("{}::DATE", "2025-04-10"),
            postgres_expr!("{}::INTERVAL", "1 day"),
        ],
    );

    let rows: Vec<DailyRevenue> = check_and_run(
        &PostgresSelect::new()
            .with_source(postgres_expr!("{} AS d(day)", (series)))
            .with_expression(ident("day").dot_of("d"), None)
            .with_expression(
                Fx::new(
                    "coalesce",
                    [
                        Fx::new("count", [ident("id").dot_of("o").expr()]).expr(),
                        postgres_expr!("{}", 0i64),
                    ],
                ),
                Some("order_count".into()),
            )
            .with_expression(
                Fx::new(
                    "coalesce",
                    [
                        Fx::new("sum", [ident("total").dot_of("o").expr()]).expr(),
                        postgres_expr!("{}", 0i64),
                    ],
                ),
                Some("daily_revenue".into()),
            )
            .with_join(PostgresSelectJoin::left(
                "orders",
                "o",
                postgres_expr!(
                    "{}::DATE = {}",
                    (ident("created_at").dot_of("o")),
                    (ident("day").dot_of("d"))
                ),
            ))
            .with_group_by(ident("day").dot_of("d"))
            .with_order(ident("day").dot_of("d"), Order::Asc),
        concat!(
            r#"SELECT "d"."day", "#,
            r#"COALESCE(COUNT("o"."id"), 0) AS "order_count", "#,
            r#"COALESCE(SUM("o"."total"), 0) AS "daily_revenue" "#,
            r#"FROM GENERATE_SERIES('2025-04-01'::DATE, '2025-04-10'::DATE, '1 day'::INTERVAL) AS d(day) "#,
            r#"LEFT JOIN "orders" AS "o" ON "o"."created_at"::DATE = "d"."day" "#,
            r#"GROUP BY "d"."day" "#,
            r#"ORDER BY "d"."day""#,
        ),
    )
    .await;

    // 10 days from April 1-10
    assert_eq!(rows.len(), 10);

    // April 1: Jake's order ($420)
    assert_eq!(rows[0].order_count, 1);
    assert_eq!(rows[0].daily_revenue, 420.0);

    // April 4: no orders
    assert_eq!(rows[3].order_count, 0);
    assert_eq!(rows[3].daily_revenue, 0.0);
}

// -- ---------------------------------------------------------------------------
// -- 6. Array aggregation — array_agg, string_agg, FILTER
// -- Features: array_agg(DISTINCT ... ORDER BY), string_agg, FILTER (WHERE)
// -- Expected: per-category product stats
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CategoryAgg {
    category: String,
    total: i64,
    product_names: Vec<String>,
    names_by_price_desc: String,
    featured_count: i64,
    sale_count: i64,
}

#[tokio::test]
async fn test_pg_q6_array_aggregation() {
    use vantage_sql::primitives::fx::Fx;

    let name = ident("name").dot_of("p");
    let tags = ident("tags").dot_of("p");

    let rows: Vec<CategoryAgg> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("products", "p")
            .with_expression(ident("category").dot_of("p"), None)
            .with_expression(
                Fx::new("count", [postgres_expr!("*")]),
                Some("total".into()),
            )
            .with_expression(
                postgres_expr!(
                    "array_agg(DISTINCT {} ORDER BY {})",
                    (name.clone()),
                    (name.clone())
                ),
                Some("product_names".into()),
            )
            .with_expression(
                postgres_expr!(
                    "string_agg({}, {} ORDER BY {} DESC)",
                    (name),
                    ", ",
                    (ident("price").dot_of("p"))
                ),
                Some("names_by_price_desc".into()),
            )
            .with_expression(
                postgres_expr!(
                    "COUNT(*) FILTER (WHERE {} @> ARRAY[{}])",
                    (tags.clone()),
                    "featured"
                ),
                Some("featured_count".into()),
            )
            .with_expression(
                postgres_expr!("COUNT(*) FILTER (WHERE {} @> ARRAY[{}])", (tags), "sale"),
                Some("sale_count".into()),
            )
            .with_group_by(ident("category").dot_of("p"))
            .with_order(ident("total"), Order::Desc),
        concat!(
            r#"SELECT "p"."category", COUNT(*) AS "total", "#,
            r#"array_agg(DISTINCT "p"."name" ORDER BY "p"."name") AS "product_names", "#,
            r#"string_agg("p"."name", ', ' ORDER BY "p"."price" DESC) AS "names_by_price_desc", "#,
            r#"COUNT(*) FILTER (WHERE "p"."tags" @> ARRAY['featured']) AS "featured_count", "#,
            r#"COUNT(*) FILTER (WHERE "p"."tags" @> ARRAY['sale']) AS "sale_count" "#,
            r#"FROM "products" AS "p" "#,
            r#"GROUP BY "p"."category" "#,
            r#"ORDER BY "total" DESC"#,
        ),
    )
    .await;

    assert_eq!(rows.len(), 5);

    // electronics: 7 products, 3 featured, 3 on sale
    assert_eq!(rows[0].category, "electronics");
    assert_eq!(rows[0].total, 7);
    assert_eq!(rows[0].featured_count, 3);
    assert_eq!(rows[0].sale_count, 3);
    assert_eq!(rows[0].product_names[0], "Gadget Pro Max");

    // furniture: 2 products, 1 featured
    assert_eq!(rows[1].category, "furniture");
    assert_eq!(rows[1].total, 2);
    assert_eq!(rows[1].featured_count, 1);
}

// -- ---------------------------------------------------------------------------
// -- 7. GROUPING SETS / ROLLUP — multi-level aggregation
// -- Features: ROLLUP, GROUPING() function, sub-totals and grand total
// -- Expected: revenue broken down by status + month, with rollup totals
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RollupRevenue {
    status: String,
    month: Option<String>,
    order_count: i64,
    revenue: f64,
}

#[tokio::test]
async fn test_pg_q7_rollup() {
    use vantage_sql::primitives::case::Case;
    use vantage_sql::primitives::fx::Fx;

    let status = ident("status").dot_of("o");
    let date_trunc = postgres_expr!("DATE_TRUNC('month', {})", (ident("created_at").dot_of("o")));
    let grouping_status = Fx::new("grouping", [status.expr()]);
    let grouping_month = Fx::new("grouping", [date_trunc.expr()]);

    let rows: Vec<RollupRevenue> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("orders", "o")
            .with_expression(
                Case::new()
                    .when(grouping_status.clone().eq(1i64), "** ALL **")
                    .else_(status.clone()),
                Some("status".into()),
            )
            .with_expression(
                Case::new()
                    .when(grouping_month.eq(1i64), postgres_expr!("NULL"))
                    .else_(date_trunc.clone()),
                Some("month".into()),
            )
            .with_expression(
                Fx::new("count", [postgres_expr!("*")]),
                Some("order_count".into()),
            )
            .with_expression(
                Fx::new("sum", [ident("total").dot_of("o").expr()]),
                Some("revenue".into()),
            )
            .with_condition(status.ne("cancelled"))
            .with_group_by(postgres_expr!("ROLLUP ({}, {})", (status.clone()), (date_trunc)))
            .with_order(grouping_status, Order::Asc)
            .with_order(status, Order::Asc)
            .with_order(ident("month"), Order::Asc.nulls_last()),
        concat!(
            r#"SELECT CASE WHEN GROUPING("o"."status") = 1 THEN '** ALL **' ELSE "o"."status" END AS "status", "#,
            r#"CASE WHEN GROUPING(DATE_TRUNC('month', "o"."created_at")) = 1 THEN NULL ELSE DATE_TRUNC('month', "o"."created_at") END AS "month", "#,
            r#"COUNT(*) AS "order_count", "#,
            r#"SUM("o"."total") AS "revenue" "#,
            r#"FROM "orders" AS "o" "#,
            r#"WHERE "o"."status" != 'cancelled' "#,
            r#"GROUP BY ROLLUP ("o"."status", DATE_TRUNC('month', "o"."created_at")) "#,
            r#"ORDER BY GROUPING("o"."status"), "o"."status", "month" NULLS LAST"#,
        ),
    )
    .await;

    assert_eq!(rows.len(), 10);

    // Grand total row: "** ALL **" with NULL month
    let grand_total = rows.iter().find(|r| r.status == "** ALL **").unwrap();
    assert_eq!(grand_total.month, None);
    assert_eq!(grand_total.order_count, 13);
    assert_eq!(grand_total.revenue, 4490.5);

    // completed subtotal: 8 orders
    let completed_sub = rows
        .iter()
        .find(|r| r.status == "completed" && r.month.is_none())
        .unwrap();
    assert_eq!(completed_sub.order_count, 8);
}

// -- ---------------------------------------------------------------------------
// -- 8. Date/time — INTERVAL arithmetic, DATE_TRUNC, EXTRACT, AGE
// -- Features: INTERVAL literal, AGE(), EXTRACT(EPOCH/DOW), DATE_TRUNC, NOW()
// -- Expected: active users with computed tenure and cohort labels
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UserTenure {
    name: String,
    signup_day_of_week: f64,
    cohort: String,
}

#[tokio::test]
async fn test_pg_q8_date_time() {
    use vantage_sql::primitives::case::Case;
    use vantage_sql::primitives::fx::Fx;
    use vantage_sql::primitives::interval::Interval;

    let created = ident("created_at").dot_of("u");
    let now = Fx::new("now", Vec::<vantage_expressions::Expression<_>>::new());
    let age = postgres_expr!("AGE({}, {})", (now), (created.clone()));

    let rows: Vec<UserTenure> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(
                postgres_expr!("EXTRACT(DOW FROM {})", (created.clone())),
                Some("signup_day_of_week".into()),
            )
            .with_expression(
                Case::new()
                    .when(age.clone().gte(Interval::new(1, "year")), "veteran")
                    .when(age.clone().gte(Interval::new(6, "month")), "established")
                    .else_(postgres_expr!("{}", "new")),
                Some("cohort".into()),
            )
            .with_condition(ident("is_active").dot_of("u").eq(true))
            .with_order(created, Order::Asc)
            .with_limit(Some(5), None),
        concat!(
            r#"SELECT "u"."name", "#,
            r#"EXTRACT(DOW FROM "u"."created_at") AS "signup_day_of_week", "#,
            r#"CASE WHEN AGE(NOW(), "u"."created_at") >= INTERVAL '1 year' THEN 'veteran' "#,
            r#"WHEN AGE(NOW(), "u"."created_at") >= INTERVAL '6 month' THEN 'established' "#,
            r#"ELSE 'new' END AS "cohort" "#,
            r#"FROM "users" AS "u" "#,
            r#"WHERE "u"."is_active" = true "#,
            r#"ORDER BY "u"."created_at" "#,
            r#"LIMIT 5"#,
        ),
    )
    .await;

    assert_eq!(rows.len(), 5);

    // Alice signed up Jan 15, 2024 (Monday = DOW 1), over 1 year ago
    assert_eq!(rows[0].name, "Alice Chen");
    assert_eq!(rows[0].signup_day_of_week, 1.0);
    assert_eq!(rows[0].cohort, "veteran");

    // All first 5 users are veterans (signed up 2024)
    assert!(rows.iter().all(|r| r.cohort == "veteran"));
}

// -- ---------------------------------------------------------------------------
// -- 9. ENUM ordering + casting
// -- Features: enum comparison (>=), enum::TEXT cast, CASE with enum values
// -- Expected: high/critical tickets with priority rank
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TicketPriority {
    id: i64,
    title: String,
    priority_text: String,
    status_text: String,
    priority_rank: i64,
}

#[tokio::test]
async fn test_pg_q9_enum() {
    use vantage_sql::primitives::case::Case;

    let priority = ident("priority").dot_of("t");

    let rows: Vec<TicketPriority> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("tickets", "t")
            .with_expression(ident("id").dot_of("t"), None)
            .with_expression(ident("title").dot_of("t"), None)
            .with_expression(priority.cast("TEXT"), Some("priority_text".into()))
            .with_expression(
                ident("status").dot_of("t").cast("TEXT"),
                Some("status_text".into()),
            )
            .with_expression(
                {
                    let p = priority.cast("TEXT");
                    Case::new()
                        .when(p.clone().eq("critical"), postgres_expr!("{}", 1i64))
                        .when(p.clone().eq("high"), postgres_expr!("{}", 2i64))
                        .when(p.clone().eq("medium"), postgres_expr!("{}", 3i64))
                        .when(p.eq("low"), postgres_expr!("{}", 4i64))
                },
                Some("priority_rank".into()),
            )
            .with_condition(postgres_expr!("{} >= 'high'", (priority)))
            .with_order(priority, Order::Desc)
            .with_order(ident("updated_at").dot_of("t"), Order::Desc),
        concat!(
            r#"SELECT "t"."id", "t"."title", "#,
            r#"CAST("t"."priority" AS TEXT) AS "priority_text", "#,
            r#"CAST("t"."status" AS TEXT) AS "status_text", "#,
            r#"CASE WHEN CAST("t"."priority" AS TEXT) = 'critical' THEN 1 "#,
            r#"WHEN CAST("t"."priority" AS TEXT) = 'high' THEN 2 "#,
            r#"WHEN CAST("t"."priority" AS TEXT) = 'medium' THEN 3 "#,
            r#"WHEN CAST("t"."priority" AS TEXT) = 'low' THEN 4 END AS "priority_rank" "#,
            r#"FROM "tickets" AS "t" "#,
            r#"WHERE "t"."priority" >= 'high' "#,
            r#"ORDER BY "t"."priority" DESC, "t"."updated_at" DESC"#,
        ),
    )
    .await;

    // critical (1) and high (3, 6) — priority >= 'high' in enum order
    assert_eq!(rows.len(), 3);

    // First: critical ticket (#1)
    assert_eq!(rows[0].id, 1);
    assert_eq!(rows[0].priority_text, "critical");
    assert_eq!(rows[0].priority_rank, 1);

    // Next two: high tickets, ordered by updated_at DESC
    assert_eq!(rows[1].priority_text, "high");
    assert_eq!(rows[2].priority_text, "high");
    assert_eq!(rows[1].priority_rank, 2);
}

// -- ---------------------------------------------------------------------------
// -- 10. DATERANGE operators — containment, overlap, bounds
// -- Features: && (overlap), lower(), upper(), daterange(), date arithmetic
// -- Expected: bookings overlapping April 7-10
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Booking {
    id: i64,
    room: String,
    booked_by: String,
    check_in: String,
    check_out: String,
    duration_days: i32,
}

#[tokio::test]
async fn test_pg_q10_daterange() {
    use vantage_sql::primitives::fx::Fx;

    let during = ident("during").dot_of("b");

    let rows: Vec<Booking> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("bookings", "b")
            .with_expression(ident("id").dot_of("b"), None)
            .with_expression(ident("room").dot_of("b"), None)
            .with_expression(ident("name").dot_of("u"), Some("booked_by".into()))
            .with_expression(Fx::new("lower", [during.expr()]), Some("check_in".into()))
            .with_expression(Fx::new("upper", [during.expr()]), Some("check_out".into()))
            .with_expression(
                Fx::new("upper", [during.expr()]).expr() - Fx::new("lower", [during.expr()]).expr(),
                Some("duration_days".into()),
            )
            .with_join(PostgresSelectJoin::inner(
                "users",
                "u",
                ident("id").dot_of("u").eq(ident("user_id").dot_of("b")),
            ))
            .with_condition(postgres_expr!(
                "{} && daterange('2025-04-07', '2025-04-10')",
                (during)
            ))
            .with_order(ident("room").dot_of("b"), Order::Asc)
            .with_order(Fx::new("lower", [during.expr()]), Order::Asc),
        concat!(
            r#"SELECT "b"."id", "b"."room", "#,
            r#""u"."name" AS "booked_by", "#,
            r#"LOWER("b"."during") AS "check_in", "#,
            r#"UPPER("b"."during") AS "check_out", "#,
            r#"UPPER("b"."during") - LOWER("b"."during") AS "duration_days" "#,
            r#"FROM "bookings" AS "b" "#,
            r#"INNER JOIN "users" AS "u" ON "u"."id" = "b"."user_id" "#,
            r#"WHERE "b"."during" && daterange('2025-04-07', '2025-04-10') "#,
            r#"ORDER BY "b"."room", LOWER("b"."during")"#,
        ),
    )
    .await;

    // 6 bookings overlap April 7-10
    assert_eq!(rows.len(), 6);

    // Room A: Alice (1 day), Bob (1 day), Alice (2 days)
    assert_eq!(rows[0].room, "Room A");
    assert_eq!(rows[0].booked_by, "Alice Chen");
    assert_eq!(rows[0].duration_days, 1);
    assert_eq!(rows[2].booked_by, "Alice Chen");
    assert_eq!(rows[2].duration_days, 2);

    // Room C: Leo (7 days)
    assert_eq!(rows[5].room, "Room C");
    assert_eq!(rows[5].booked_by, "Leo Russo");
    assert_eq!(rows[5].duration_days, 7);
}

// -- ---------------------------------------------------------------------------
// -- 11. UNNEST WITH ORDINALITY — expand arrays preserving position
// -- Features: CROSS JOIN LATERAL, UNNEST, WITH ORDINALITY, column alias
// -- Expected: each user's skills as individual rows with position
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UserSkillRow {
    name: String,
    skill: String,
    skill_position: i64,
}

#[tokio::test]
async fn test_pg_q11_unnest_ordinality() {
    use vantage_sql::primitives::fx::Fx;

    let skills = ident("skills").dot_of("u");

    let rows: Vec<UserSkillRow> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("value").dot_of("skill"), Some("skill".into()))
            .with_expression(ident("pos").dot_of("skill"), Some("skill_position".into()))
            .with_join(PostgresSelectJoin::cross_lateral_raw(postgres_expr!(
                "UNNEST({}) WITH ORDINALITY AS skill(value, pos)",
                (skills.clone())
            )))
            .with_condition(postgres_expr!(
                "{} > {}",
                (Fx::new("array_length", [skills.expr(), postgres_expr!("{}", 1i32)])),
                0i32
            ))
            .with_order(ident("name").dot_of("u"), Order::Asc)
            .with_order(ident("pos").dot_of("skill"), Order::Asc),
        concat!(
            r#"SELECT "u"."name", "skill"."value" AS "skill", "skill"."pos" AS "skill_position" "#,
            r#"FROM "users" AS "u" "#,
            r#"CROSS JOIN LATERAL UNNEST("u"."skills") WITH ORDINALITY AS skill(value, pos) "#,
            r#"WHERE ARRAY_LENGTH("u"."skills", 1) > 0 "#,
            r#"ORDER BY "u"."name", "skill"."pos""#,
        ),
    )
    .await;

    // 11 users with skills (Karen has empty array), multiple rows each
    assert!(!rows.is_empty());

    // Alice has 3 skills: rust, postgresql, kubernetes
    let alice: Vec<&UserSkillRow> = rows.iter().filter(|r| r.name == "Alice Chen").collect();
    assert_eq!(alice.len(), 3);
    assert_eq!(alice[0].skill, "rust");
    assert_eq!(alice[0].skill_position, 1);
    assert_eq!(alice[2].skill, "kubernetes");
    assert_eq!(alice[2].skill_position, 3);
}

// -- ---------------------------------------------------------------------------
// -- 12. Recursive CTE with PG-specific cast
// -- Features: WITH RECURSIVE, ::TEXT cast, || concat, depth tracking
// -- Expected: department tree with breadcrumb path
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct DeptTree {
    id: i32,
    name: String,
    depth: i64,
    path: String,
}

#[tokio::test]
async fn test_pg_q12_recursive_cte() {
    use vantage_sql::concat_sql;
    use vantage_sql::primitives::union::Union;

    let base = PostgresSelect::new()
        .with_source("departments")
        .with_field("id")
        .with_field("name")
        .with_expression(ident("parent_id"), None)
        .with_expression(postgres_expr!("0"), None)
        .with_expression(ident("name").cast("TEXT"), None)
        .with_condition(postgres_expr!("{} IS NULL", (ident("parent_id"))));

    let recursive = PostgresSelect::new()
        .with_source_as("departments", "d")
        .with_expression(ident("id").dot_of("d"), None)
        .with_expression(ident("name").dot_of("d"), None)
        .with_expression(ident("parent_id").dot_of("d"), None)
        .with_expression(
            ident("depth").dot_of("dt").expr() + postgres_expr!("1"),
            None,
        )
        .with_expression(
            concat_sql!(ident("path").dot_of("dt"), " > ", ident("name").dot_of("d")),
            None,
        )
        .with_join(PostgresSelectJoin::inner(
            "dept_tree",
            "dt",
            ident("id").dot_of("dt").eq(ident("parent_id").dot_of("d")),
        ));

    let rows: Vec<DeptTree> = check_and_run(
        &PostgresSelect::new()
            .with_cte(
                "dept_tree(id, name, parent_id, depth, path)",
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
            r#"WITH RECURSIVE dept_tree(id, name, parent_id, depth, path) AS "#,
            r#"(SELECT "id", "name", "parent_id", 0, CAST("name" AS TEXT) "#,
            r#"FROM "departments" WHERE "parent_id" IS NULL "#,
            r#"UNION ALL "#,
            r#"SELECT "d"."id", "d"."name", "d"."parent_id", "dt"."depth" + 1, "#,
            r#""dt"."path" || ' > ' || "d"."name" "#,
            r#"FROM "departments" AS "d" "#,
            r#"INNER JOIN "dept_tree" AS "dt" ON "dt"."id" = "d"."parent_id") "#,
            r#"SELECT "id", "name", "depth", "path" "#,
            r#"FROM "dept_tree" "#,
            r#"ORDER BY "path""#,
        ),
    )
    .await;

    assert_eq!(rows.len(), 8);

    let roots: Vec<&DeptTree> = rows.iter().filter(|r| r.depth == 0).collect();
    assert_eq!(roots.len(), 3);

    let backend = rows.iter().find(|r| r.name == "Backend").unwrap();
    assert_eq!(backend.depth, 1);
    assert_eq!(backend.path, "Engineering > Backend");
}

// -- ---------------------------------------------------------------------------
// -- 13. Window functions with PG-specific features
// -- Features: DENSE_RANK, named WINDOW, ROWS frame, AVG window
// -- Expected: salary distribution stats per department
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SalaryStats {
    name: String,
    department: String,
    salary: f64,
    salary_dense_rank: i64,
    running_total: f64,
    dept_avg_salary: f64,
}

#[tokio::test]
async fn test_pg_q13_window_functions() {
    use vantage_sql::primitives::fx::Fx;
    use vantage_sql::primitives::select::window::Window;

    let dept_id = ident("department_id").dot_of("u");
    let salary = ident("salary").dot_of("u");

    let rows: Vec<SalaryStats> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(ident("name").dot_of("d"), Some("department".into()))
            .with_expression(salary.clone(), None)
            .with_expression(
                Window::named("dept_sal").apply(Fx::new("dense_rank", vec![])),
                Some("salary_dense_rank".into()),
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
                Window::named("dept_sal").apply(Fx::new("avg", [salary.expr()])),
                Some("dept_avg_salary".into()),
            )
            .with_join(PostgresSelectJoin::inner(
                "departments",
                "d",
                ident("id").dot_of("d").eq(dept_id.clone()),
            ))
            .with_condition(ident("is_active").dot_of("u").eq(true))
            .with_window(
                "dept_sal",
                Window::new()
                    .partition_by(dept_id)
                    .order_by(salary, Order::Desc),
            )
            .with_order(ident("department"), Order::Asc)
            .with_order(ident("salary_dense_rank"), Order::Asc),
        concat!(
            r#"SELECT "u"."name", "d"."name" AS "department", "u"."salary", "#,
            r#"DENSE_RANK() OVER dept_sal AS "salary_dense_rank", "#,
            r#"SUM("u"."salary") OVER (PARTITION BY "u"."department_id" ORDER BY "u"."salary" DESC "#,
            r#"ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS "running_total", "#,
            r#"AVG("u"."salary") OVER dept_sal AS "dept_avg_salary" "#,
            r#"FROM "users" AS "u" "#,
            r#"INNER JOIN "departments" AS "d" ON "d"."id" = "u"."department_id" "#,
            r#"WHERE "u"."is_active" = true "#,
            r#"WINDOW dept_sal AS (PARTITION BY "u"."department_id" ORDER BY "u"."salary" DESC) "#,
            r#"ORDER BY "department", "salary_dense_rank""#,
        ),
    )
    .await;

    assert_eq!(rows.len(), 10);

    // Backend: Alice (rank 1, 120k), Bob (rank 2, 95k)
    let backend: Vec<&SalaryStats> = rows.iter().filter(|r| r.department == "Backend").collect();
    assert_eq!(backend.len(), 2);
    assert_eq!(backend[0].name, "Alice Chen");
    assert_eq!(backend[0].salary_dense_rank, 1);
    assert_eq!(backend[0].running_total, 120000.0);
    assert_eq!(backend[1].running_total, 215000.0);
}

// -- ---------------------------------------------------------------------------
// -- 14. JSONB aggregation — jsonb_build_object, jsonb_agg, correlated subquery
// -- Features: jsonb_build_object(), to_jsonb(), jsonb_agg(DISTINCT), subquery
// -- Expected: per-user summary as a single JSONB document
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UserProfile {
    name: String,
    user_profile: serde_json::Value,
}

#[tokio::test]
async fn test_pg_q14_jsonb_aggregation() {
    use vantage_sql::primitives::fx::Fx;

    let o_total = ident("total").dot_of("o");
    let o_status = ident("status").dot_of("o");

    let order_summary_subquery = PostgresSelect::new()
        .with_source_as("orders", "o")
        .with_expression(
            Fx::new(
                "jsonb_build_object",
                [
                    postgres_expr!("{}", "count"),
                    Fx::new("count", [postgres_expr!("*")]).expr(),
                    postgres_expr!("{}", "total"),
                    Fx::new(
                        "coalesce",
                        [
                            Fx::new("sum", [o_total.expr()]).expr(),
                            postgres_expr!("{}", 0i64),
                        ],
                    )
                    .expr(),
                    postgres_expr!("{}", "statuses"),
                    postgres_expr!("jsonb_agg(DISTINCT {})", (o_status)),
                ],
            ),
            None,
        )
        .with_condition(ident("user_id").dot_of("o").eq(ident("id").dot_of("u")));

    let rows: Vec<UserProfile> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("users", "u")
            .with_expression(ident("name").dot_of("u"), None)
            .with_expression(
                Fx::new(
                    "jsonb_build_object",
                    [
                        postgres_expr!("{}", "user_id"),
                        ident("id").dot_of("u").expr(),
                        postgres_expr!("{}", "email"),
                        ident("email").dot_of("u").expr(),
                        postgres_expr!("{}", "role"),
                        ident("role").dot_of("u").expr(),
                        postgres_expr!("{}", "skills"),
                        Fx::new("to_jsonb", [ident("skills").dot_of("u").expr()]).expr(),
                        postgres_expr!("{}", "order_summary"),
                        postgres_expr!("({})", (order_summary_subquery)),
                    ],
                ),
                Some("user_profile".into()),
            )
            .with_condition(ident("is_active").dot_of("u").eq(true))
            .with_order(ident("name").dot_of("u"), Order::Asc)
            .with_limit(Some(3), None),
        concat!(
            r#"SELECT "u"."name", "#,
            r#"JSONB_BUILD_OBJECT('user_id', "u"."id", 'email', "u"."email", "#,
            r#"'role', "u"."role", 'skills', TO_JSONB("u"."skills"), "#,
            r#"'order_summary', (SELECT JSONB_BUILD_OBJECT("#,
            r#"'count', COUNT(*), "#,
            r#"'total', COALESCE(SUM("o"."total"), 0), "#,
            r#"'statuses', jsonb_agg(DISTINCT "o"."status")) "#,
            r#"FROM "orders" AS "o" "#,
            r#"WHERE "o"."user_id" = "u"."id")) AS "user_profile" "#,
            r#"FROM "users" AS "u" "#,
            r#"WHERE "u"."is_active" = true "#,
            r#"ORDER BY "u"."name" "#,
            r#"LIMIT 3"#,
        ),
    )
    .await;

    assert_eq!(rows.len(), 3);

    // Alice's profile
    let alice = &rows[0];
    assert_eq!(alice.name, "Alice Chen");
    let profile = &alice.user_profile;
    assert_eq!(profile["role"], "admin");
    assert_eq!(profile["email"], "alice@example.com");
    assert_eq!(profile["order_summary"]["count"], 4);
}

// -- ---------------------------------------------------------------------------
// -- 15. INET operators + ILIKE + FULL OUTER JOIN + COALESCE + NULLIF
// -- Features: <<= (subnet containment), ILIKE, FULL OUTER JOIN, COALESCE, NULLIF
// -- Expected: user network info with department, including orphaned rows
// -- ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UserNetwork {
    name: String,
    department: String,
    network_class: String,
    notable_role: Option<String>,
}

#[tokio::test]
async fn test_pg_q15_inet_ilike_full_join() {
    use vantage_sql::primitives::case::Case;
    use vantage_sql::primitives::fx::Fx;

    let ip = ident("last_login_ip").dot_of("u");

    let rows: Vec<UserNetwork> = check_and_run(
        &PostgresSelect::new()
            .with_source_as("users", "u")
            .with_expression(
                Fx::new(
                    "coalesce",
                    [
                        ident("name").dot_of("u").expr(),
                        postgres_expr!("{}", "(no user)"),
                    ],
                ),
                Some("name".into()),
            )
            .with_expression(
                Fx::new(
                    "coalesce",
                    [
                        ident("name").dot_of("d").expr(),
                        postgres_expr!("{}", "(no department)"),
                    ],
                ),
                Some("department".into()),
            )
            .with_expression(
                Case::new()
                    .when(
                        postgres_expr!("{} <<= '192.168.0.0/16'::INET", (ip.clone())),
                        "private-class-c",
                    )
                    .when(
                        postgres_expr!("{} <<= '10.0.0.0/8'::INET", (ip.clone())),
                        "private-class-a",
                    )
                    .when(
                        postgres_expr!("{} <<= '172.16.0.0/12'::INET", (ip.clone())),
                        "private-class-b",
                    )
                    .when(postgres_expr!("{} IS NULL", (ip)), "unknown")
                    .else_(postgres_expr!("{}", "public")),
                Some("network_class".into()),
            )
            .with_expression(
                Fx::new(
                    "nullif",
                    [
                        ident("role").dot_of("u").expr(),
                        postgres_expr!("{}", "viewer"),
                    ],
                ),
                Some("notable_role".into()),
            )
            .with_join(PostgresSelectJoin::full_outer(
                "departments",
                "d",
                ident("id")
                    .dot_of("d")
                    .eq(ident("department_id").dot_of("u")),
            ))
            .with_condition(postgres_expr!(
                "{} ILIKE {} OR {} ILIKE {}",
                (ident("name").dot_of("u")),
                "%a%",
                (ident("name").dot_of("d")),
                "%design%"
            ))
            .with_order(ident("name").dot_of("u"), Order::Asc.nulls_last()),
        concat!(
            r#"SELECT COALESCE("u"."name", '(no user)') AS "name", "#,
            r#"COALESCE("d"."name", '(no department)') AS "department", "#,
            r#"CASE WHEN "u"."last_login_ip" <<= '192.168.0.0/16'::INET THEN 'private-class-c' "#,
            r#"WHEN "u"."last_login_ip" <<= '10.0.0.0/8'::INET THEN 'private-class-a' "#,
            r#"WHEN "u"."last_login_ip" <<= '172.16.0.0/12'::INET THEN 'private-class-b' "#,
            r#"WHEN "u"."last_login_ip" IS NULL THEN 'unknown' "#,
            r#"ELSE 'public' END AS "network_class", "#,
            r#"NULLIF("u"."role", 'viewer') AS "notable_role" "#,
            r#"FROM "users" AS "u" "#,
            r#"FULL OUTER JOIN "departments" AS "d" ON "d"."id" = "u"."department_id" "#,
            r#"WHERE "u"."name" ILIKE '%a%' OR "d"."name" ILIKE '%design%' "#,
            r#"ORDER BY "u"."name" NULLS LAST"#,
        ),
    )
    .await;

    assert_eq!(rows.len(), 10);

    // Alice: private-class-c, admin
    let alice = rows.iter().find(|r| r.name == "Alice Chen").unwrap();
    assert_eq!(alice.network_class, "private-class-c");
    assert_eq!(alice.notable_role, Some("admin".to_string()));

    // Frank: no IP → unknown, viewer → notable_role is None
    let frank = rows.iter().find(|r| r.name == "Frank Lee").unwrap();
    assert_eq!(frank.network_class, "unknown");
    assert_eq!(frank.notable_role, None);

    // Karen: no department
    let karen = rows.iter().find(|r| r.name == "Karen Hill").unwrap();
    assert_eq!(karen.department, "(no department)");
}
