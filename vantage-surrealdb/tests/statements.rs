//! Comprehensive integration tests for SurrealDB statement builders.
//!
//! Covers SurrealSelect, SurrealInsert, SurrealUpdate, SurrealDelete
//! including all builder methods, edge cases, and round-trip verification.

use std::sync::atomic::{AtomicU32, Ordering};

use vantage_expressions::{ExprDataSource, Expressive, Selectable};
use vantage_surrealdb::statements::delete::SurrealDelete;
use vantage_surrealdb::statements::insert::SurrealInsert;
use vantage_surrealdb::statements::select::SurrealSelect;

use vantage_surrealdb::statements::select::target::Target;
use vantage_surrealdb::statements::update::SurrealUpdate;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::{AnySurrealType, SurrealType};
use vantage_surrealdb::{identifier::Identifier, surreal_expr};

static TEST_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn get_db() -> SurrealDB {
    let n = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dsn = format!("cbor://root:root@localhost:8000/bakery/stmt_test_{}", n);
    let client = surreal_client::SurrealConnection::dsn(&dsn)
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

async fn cleanup(db: &SurrealDB, tables: &[&str]) {
    for t in tables {
        db.execute(&surreal_expr!(&format!("DELETE {}", t)))
            .await
            .ok();
    }
}

async fn read_value<T: SurrealType>(db: &SurrealDB, record: &str, field: &str) -> T {
    let result = db
        .execute(&surreal_expr!(&format!(
            "SELECT VALUE {} FROM ONLY {}",
            field, record
        )))
        .await
        .expect("read_value failed");
    result
        .try_get::<T>()
        .expect("read_value type conversion failed")
}

async fn count_records(db: &SurrealDB, table: &str) -> i64 {
    let result = db
        .execute(&surreal_expr!(&format!(
            "RETURN count(SELECT VALUE id FROM {})",
            table
        )))
        .await
        .expect("count_records failed");
    result.try_get::<i64>().unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealSelect — query rendering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn select_basic_fields() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("users"))])
        .with_field("name")
        .with_field("email");

    assert_eq!(select.preview(), "SELECT name, email FROM users");
}

#[test]
fn select_star() {
    let select = SurrealSelect::new().from(vec![Target::new(Identifier::new("products"))]);
    assert_eq!(select.preview(), "SELECT * FROM products");
}

#[test]
fn select_with_alias() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("users"))])
        .with_expression(surreal_expr!("name"), Some("user_name".to_string()))
        .with_expression(surreal_expr!("math::sum(score)"), Some("total".to_string()));

    let p = select.preview();
    assert!(p.contains("name AS user_name"));
    assert!(p.contains("math::sum(score) AS total"));
}

#[test]
fn select_where_single() {
    let mut select = SurrealSelect::new();
    select.set_source("users", None);
    select.add_field("name".to_string());
    select.add_where_condition(surreal_expr!("active = {}", true));

    assert_eq!(
        select.preview(),
        "SELECT name FROM users WHERE active = true"
    );
}

#[test]
fn select_where_multiple() {
    let mut select = SurrealSelect::new();
    select.set_source("orders", None);
    select.add_where_condition(surreal_expr!("status = {}", "pending"));
    select.add_where_condition(surreal_expr!("total > {}", 100i64));

    let p = select.preview();
    assert!(p.contains("WHERE status = \"pending\" AND total > 100"));
}

#[test]
fn select_order_by() {
    let mut select = SurrealSelect::new();
    select.set_source("products", None);
    select.add_field("name".to_string());
    select.add_order_by(surreal_expr!("price"), false); // DESC

    let p = select.preview();
    assert!(p.contains("ORDER BY price DESC"));
}

#[test]
fn select_order_by_asc() {
    let mut select = SurrealSelect::new();
    select.set_source("products", None);
    select.add_order_by(surreal_expr!("name"), true);

    let p = select.preview();
    // SurrealDB may omit ASC (it's the default) — just verify field is there
    assert!(p.contains("ORDER BY name"), "got: {}", p);
}

#[test]
fn select_group_by() {
    let mut select = SurrealSelect::new();
    select.set_source("orders", None);
    select.add_expression(surreal_expr!("status"), None);
    select.add_expression(surreal_expr!("count()"), Some("cnt".to_string()));
    select.add_group_by(surreal_expr!("status"));

    let p = select.preview();
    assert!(p.contains("GROUP BY status"));
    assert!(p.contains("count() AS cnt"));
}

#[test]
fn select_limit_and_skip() {
    let mut select = SurrealSelect::new();
    select.set_source("logs", None);
    select.set_limit(Some(10), Some(20));

    let p = select.preview();
    assert!(p.contains("LIMIT 10"));
    assert!(p.contains("START 20"));
    assert_eq!(select.get_limit(), Some(10));
    assert_eq!(select.get_skip(), Some(20));
}

#[test]
fn select_limit_only() {
    let mut select = SurrealSelect::new();
    select.set_source("logs", None);
    select.set_limit(Some(5), None);

    let p = select.preview();
    assert!(p.contains("LIMIT 5"));
    assert!(!p.contains("START"));
}

#[test]
fn select_distinct() {
    let mut select = SurrealSelect::new();
    select.set_source("events", None);
    select.add_field("type".to_string());
    select.set_distinct(true);

    assert!(select.is_distinct());
    // SurrealDB doesn't have SQL DISTINCT — verify flag is set
}

#[test]
fn select_value_mode() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("users"))])
        .with_field("name")
        .with_value();

    assert_eq!(select.preview(), "SELECT VALUE name FROM users");
}

#[test]
fn select_clear_methods() {
    let mut select = SurrealSelect::new();
    select.set_source("t", None);
    select.add_field("a".to_string());
    select.add_where_condition(surreal_expr!("x = 1"));
    select.add_order_by(surreal_expr!("a"), true);
    select.add_group_by(surreal_expr!("a"));

    assert!(select.has_fields());
    assert!(select.has_where_conditions());
    assert!(select.has_order_by());
    assert!(select.has_group_by());

    select.clear_fields();
    select.clear_where_conditions();
    select.clear_order_by();
    select.clear_group_by();

    assert!(!select.has_fields());
    assert!(!select.has_where_conditions());
    assert!(!select.has_order_by());
    assert!(!select.has_group_by());
}

#[test]
fn select_without_fields() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("t"))])
        .with_field("a")
        .with_field("b")
        .without_fields();

    assert_eq!(select.preview(), "SELECT * FROM t");
}

#[test]
fn select_only_column() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("products"))])
        .with_field("name")
        .with_field("price")
        .only_column("name");

    // only_column → SELECT VALUE name
    assert_eq!(select.preview(), "SELECT VALUE name FROM products");
}

#[test]
fn select_only_expression() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("products"))])
        .with_field("name")
        .only_expression(surreal_expr!("math::sum(price)"));

    assert_eq!(
        select.preview(),
        "SELECT VALUE math::sum(price) FROM products"
    );
}

#[test]
fn select_as_count() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("users"))])
        .as_count();

    let p = select.preview();
    assert!(p.contains("RETURN count("));
}

#[test]
fn select_as_sum() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("orders"))])
        .as_sum(surreal_expr!("total"));

    let p = select.preview();
    assert!(p.contains("math::sum("));
}

#[test]
fn select_as_max() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("scores"))])
        .as_max(surreal_expr!("value"));

    let p = select.preview();
    assert!(p.contains("math::max("));
}

#[test]
fn select_as_min() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("scores"))])
        .as_min(surreal_expr!("value"));

    let p = select.preview();
    assert!(p.contains("math::min("));
}

#[test]
fn select_complex_query() {
    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("order_line"))])
        .with_expression(surreal_expr!("product"), None)
        .with_expression(
            surreal_expr!("math::sum(qty * price)"),
            Some("total".to_string()),
        )
        .with_expression(surreal_expr!("count()"), Some("cnt".to_string()));

    let mut select = select;
    select.add_where_condition(surreal_expr!("order.status = {}", "confirmed"));
    select.add_group_by(surreal_expr!("product"));
    select.add_order_by(surreal_expr!("total"), false);
    select.set_limit(Some(10), None);

    let p = select.preview();
    assert!(p.contains("SELECT product"));
    assert!(p.contains("math::sum(qty * price) AS total"));
    assert!(p.contains("count() AS cnt"));
    assert!(p.contains("FROM order_line"));
    assert!(p.contains("WHERE order.status = \"confirmed\""));
    assert!(p.contains("GROUP BY product"));
    assert!(p.contains("ORDER BY total DESC"));
    assert!(p.contains("LIMIT 10"));
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealSelect — live execution
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn select_live_basic() {
    let db = get_db().await;
    cleanup(&db, &["sl_user"]).await;

    for (id, name) in [("a", "Alice"), ("b", "Bob"), ("c", "Carol")] {
        let ins = SurrealInsert::new("sl_user")
            .with_id(id)
            .with_field("name", name.to_string());
        db.execute(&ins.expr()).await.unwrap();
    }

    let select = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("sl_user"))])
        .with_value()
        .with_field("name");

    let result = db.execute(&select.expr()).await.unwrap();
    let names: Vec<String> = result.try_get().unwrap();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"Alice".to_string()));
    assert!(names.contains(&"Bob".to_string()));

    cleanup(&db, &["sl_user"]).await;
}

#[tokio::test]
async fn select_live_with_where_and_order() {
    let db = get_db().await;
    cleanup(&db, &["sl_score"]).await;

    for (id, name, score) in [("a", "Alice", 80i64), ("b", "Bob", 50), ("c", "Carol", 95)] {
        let ins = SurrealInsert::new("sl_score")
            .with_id(id)
            .with_field("name", name.to_string())
            .with_field("score", score);
        db.execute(&ins.expr()).await.unwrap();
    }

    // Score > 60, order by score DESC — use full row select (not VALUE)
    // because SurrealDB requires ORDER BY field to be in the selection
    let mut select = SurrealSelect::new();
    select.set_source("sl_score", None);
    select.add_field("name".to_string());
    select.add_field("score".to_string());
    select.add_where_condition(surreal_expr!("score > {}", 60i64));
    select.add_order_by(surreal_expr!("score"), false);

    let result = db.execute(&select.expr()).await.unwrap();
    let rows: Vec<indexmap::IndexMap<String, AnySurrealType>> = result.try_get().unwrap();
    let names: Vec<String> = rows
        .iter()
        .map(|r| r.get("name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert_eq!(names, vec!["Carol", "Alice"]);

    cleanup(&db, &["sl_score"]).await;
}

#[tokio::test]
async fn select_live_with_limit() {
    let db = get_db().await;
    cleanup(&db, &["sl_limit"]).await;

    for i in 0..10i64 {
        let ins = SurrealInsert::new("sl_limit")
            .with_id(format!("r{}", i))
            .with_field("val", i);
        db.execute(&ins.expr()).await.unwrap();
    }

    let mut select = SurrealSelect::new();
    select.set_source("sl_limit", None);
    select.set_limit(Some(3), None);

    let select = select.with_value().with_field("val");

    let result = db.execute(&select.expr()).await.unwrap();
    let vals: Vec<i64> = result.try_get().unwrap();
    assert_eq!(vals.len(), 3);

    cleanup(&db, &["sl_limit"]).await;
}

#[tokio::test]
async fn select_live_aggregation() {
    let db = get_db().await;
    cleanup(&db, &["sl_agg"]).await;

    for (id, val) in [("a", 10i64), ("b", 20), ("c", 30)] {
        let ins = SurrealInsert::new("sl_agg")
            .with_id(id)
            .with_field("val", val);
        db.execute(&ins.expr()).await.unwrap();
    }

    // count
    let count_q = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("sl_agg"))])
        .as_count();
    let count: i64 = db
        .execute(&count_q.expr())
        .await
        .unwrap()
        .try_get()
        .unwrap();
    assert_eq!(count, 3);

    // sum
    let sum_q = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("sl_agg"))])
        .as_sum(surreal_expr!("val"));
    let sum: i64 = db.execute(&sum_q.expr()).await.unwrap().try_get().unwrap();
    assert_eq!(sum, 60);

    // max
    let max_q = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("sl_agg"))])
        .as_max(surreal_expr!("val"));
    let max: i64 = db.execute(&max_q.expr()).await.unwrap().try_get().unwrap();
    assert_eq!(max, 30);

    // min
    let min_q = SurrealSelect::new()
        .from(vec![Target::new(Identifier::new("sl_agg"))])
        .as_min(surreal_expr!("val"));
    let min: i64 = db.execute(&min_q.expr()).await.unwrap().try_get().unwrap();
    assert_eq!(min, 10);

    cleanup(&db, &["sl_agg"]).await;
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealInsert — query rendering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn insert_empty_table() {
    let ins = SurrealInsert::new("things");
    assert_eq!(ins.preview(), "CREATE things");
}

#[test]
fn insert_with_id() {
    let ins = SurrealInsert::new("things").with_id("abc");
    assert_eq!(ins.preview(), "CREATE things:abc");
}

#[test]
fn insert_set_field_types() {
    let ins = SurrealInsert::new("rec")
        .with_id("t1")
        .with_field("s", "hello".to_string())
        .with_field("i", 42i64)
        .with_field("f", 3.14f64)
        .with_field("b", true);

    let p = ins.preview();
    assert!(p.contains("s = \"hello\""), "string field: {}", p);
    assert!(p.contains("i = 42"), "int field: {}", p);
    assert!(p.contains("f = 3.14"), "float field: {}", p);
    assert!(p.contains("b = true"), "bool field: {}", p);
}

#[test]
fn insert_set_any_field() {
    let val = AnySurrealType::new(99i64);
    let ins = SurrealInsert::new("rec").with_any_field("x", val);
    assert!(ins.preview().contains("x = 99"));
}

#[test]
fn insert_thing_field() {
    let ins = SurrealInsert::new("order")
        .with_id("o1")
        .with_field("customer", Thing::new("user", "alice"));

    let p = ins.preview();
    assert!(p.starts_with("CREATE order:o1 SET"));
    assert!(p.contains("customer ="));
}

#[test]
fn insert_reserved_keyword_escaping() {
    let ins = SurrealInsert::new("SELECT")
        .with_field("FROM", "val".to_string())
        .with_field("WHERE", 1i64);

    let p = ins.preview();
    assert!(p.contains("CREATE ⟨SELECT⟩ SET"), "table: {}", p);
    assert!(p.contains("⟨FROM⟩ ="), "field FROM: {}", p);
    assert!(p.contains("⟨WHERE⟩ ="), "field WHERE: {}", p);
}

#[test]
fn insert_parameterized_expression() {
    let ins = SurrealInsert::new("t")
        .with_field("a", 1i64)
        .with_field("b", 2i64);

    let expr = ins.expr();
    // target nested + 2 scalar params
    assert_eq!(expr.parameters.len(), 3);
    assert!(expr.template.contains("{}"));
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealInsert — live execution
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn insert_live_with_id() {
    let db = get_db().await;
    cleanup(&db, &["si_basic"]).await;

    let ins = SurrealInsert::new("si_basic")
        .with_id("r1")
        .with_field("name", "Test".to_string())
        .with_field("val", 123i64);

    db.execute(&ins.expr()).await.expect("insert failed");

    assert_eq!(
        read_value::<String>(&db, "si_basic:r1", "name").await,
        "Test"
    );
    assert_eq!(read_value::<i64>(&db, "si_basic:r1", "val").await, 123);

    cleanup(&db, &["si_basic"]).await;
}

#[tokio::test]
async fn insert_live_auto_id() {
    let db = get_db().await;
    cleanup(&db, &["si_auto"]).await;

    let ins = SurrealInsert::new("si_auto").with_field("label", "auto".to_string());

    db.execute(&ins.expr()).await.expect("insert failed");

    assert_eq!(count_records(&db, "si_auto").await, 1);

    cleanup(&db, &["si_auto"]).await;
}

#[tokio::test]
async fn insert_live_all_types() {
    let db = get_db().await;
    cleanup(&db, &["si_types"]).await;

    let ins = SurrealInsert::new("si_types")
        .with_id("t1")
        .with_field("str_val", "hello".to_string())
        .with_field("int_val", 42i64)
        .with_field("float_val", 2.718f64)
        .with_field("bool_val", false)
        .with_field("ref_val", Thing::new("other", "x"));

    db.execute(&ins.expr()).await.unwrap();

    assert_eq!(
        read_value::<String>(&db, "si_types:t1", "str_val").await,
        "hello"
    );
    assert_eq!(read_value::<i64>(&db, "si_types:t1", "int_val").await, 42);
    assert_eq!(
        read_value::<f64>(&db, "si_types:t1", "float_val").await,
        2.718
    );
    assert!(!read_value::<bool>(&db, "si_types:t1", "bool_val").await);

    // Thing reference stored correctly
    let ref_result = db
        .execute(&surreal_expr!("SELECT VALUE ref_val FROM ONLY si_types:t1"))
        .await
        .unwrap();
    let thing: Thing = ref_result.try_get().unwrap();
    assert_eq!(thing.table(), "other");
    assert_eq!(thing.id(), "x");

    cleanup(&db, &["si_types"]).await;
}

#[tokio::test]
async fn insert_live_multiple_records() {
    let db = get_db().await;
    cleanup(&db, &["si_multi"]).await;

    for i in 0..5i64 {
        let ins = SurrealInsert::new("si_multi")
            .with_id(format!("r{}", i))
            .with_field("idx", i);
        db.execute(&ins.expr()).await.unwrap();
    }

    assert_eq!(count_records(&db, "si_multi").await, 5);

    cleanup(&db, &["si_multi"]).await;
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealUpdate — query rendering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn update_set_basic() {
    let upd = SurrealUpdate::new(Thing::new("t", "1"))
        .with_field("name", "Alice".to_string())
        .with_field("age", 30i64);

    let p = upd.preview();
    assert!(p.starts_with("UPDATE t:1 SET"));
    assert!(p.contains("name = \"Alice\""));
    assert!(p.contains("age = 30"));
}

#[test]
fn update_set_empty() {
    let upd = SurrealUpdate::new(Thing::new("t", "1"));
    assert_eq!(upd.preview(), "UPDATE t:1");
}

#[test]
fn update_content_mode() {
    let upd = SurrealUpdate::new(Thing::new("t", "1"))
        .content()
        .with_field("x", 1i64);

    let p = upd.preview();
    assert!(p.contains("CONTENT"), "got: {}", p);
    assert!(!p.contains("SET"));
    assert!(!p.contains("MERGE"));
}

#[test]
fn update_merge_mode() {
    let upd = SurrealUpdate::new(Thing::new("t", "1"))
        .merge()
        .with_field("x", 1i64);

    let p = upd.preview();
    assert!(p.contains("MERGE"), "got: {}", p);
    assert!(!p.contains("SET"));
    assert!(!p.contains("CONTENT"));
}

#[test]
fn update_mode_switching() {
    let upd = SurrealUpdate::new(Thing::new("t", "1"))
        .content()
        .with_field("a", 1i64);
    assert!(upd.preview().contains("CONTENT"));

    let upd = upd.merge();
    assert!(upd.preview().contains("MERGE"));

    let upd = upd.set();
    assert!(upd.preview().contains("SET"));
}

#[test]
fn update_set_any_field() {
    let val = AnySurrealType::new(77i64);
    let upd = SurrealUpdate::new(Thing::new("t", "1")).with_any_field("score", val);
    assert!(upd.preview().contains("score = 77"));
}

#[test]
fn update_set_record() {
    let mut record = vantage_types::Record::new();
    record.insert("a".to_string(), AnySurrealType::new(10i64));
    record.insert("b".to_string(), AnySurrealType::new("hi".to_string()));

    let upd = SurrealUpdate::new(Thing::new("t", "1")).with_record(&record);
    let p = upd.preview();
    assert!(p.contains("a = 10"));
    assert!(p.contains("b = \"hi\""));
}

#[test]
fn update_with_thing_field() {
    let upd = SurrealUpdate::new(Thing::new("child", "c1"))
        .with_field("parent", Thing::new("parent", "p1"));

    let p = upd.preview();
    assert!(p.contains("UPDATE child:c1 SET"));
    assert!(p.contains("parent ="));
}

#[test]
fn update_parameterized_expression() {
    let upd = SurrealUpdate::new(Thing::new("t", "1"))
        .with_field("a", 1i64)
        .with_field("b", 2i64);

    let expr = upd.expr();
    assert_eq!(expr.parameters.len(), 3); // target + 2 fields
    assert!(expr.template.contains("{}"));
}

#[test]
fn update_with_arbitrary_target_expression() {
    let upd =
        SurrealUpdate::new(surreal_expr!("user WHERE active = true")).with_field("checked", true);

    let p = upd.preview();
    assert!(p.contains("UPDATE user WHERE active = true SET"));
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealUpdate — live execution
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn update_live_set() {
    let db = get_db().await;
    cleanup(&db, &["su_set"]).await;

    let ins = SurrealInsert::new("su_set")
        .with_id("r1")
        .with_field("name", "Old".to_string())
        .with_field("score", 10i64);
    db.execute(&ins.expr()).await.unwrap();

    let upd = SurrealUpdate::new(Thing::new("su_set", "r1")).with_field("score", 99i64);
    db.execute(&upd.expr()).await.unwrap();

    assert_eq!(read_value::<i64>(&db, "su_set:r1", "score").await, 99);
    // name preserved in SET mode
    assert_eq!(read_value::<String>(&db, "su_set:r1", "name").await, "Old");

    cleanup(&db, &["su_set"]).await;
}

#[tokio::test]
async fn update_live_content_replaces_all() {
    let db = get_db().await;
    cleanup(&db, &["su_content"]).await;

    let ins = SurrealInsert::new("su_content")
        .with_id("r1")
        .with_field("name", "Original".to_string())
        .with_field("score", 50i64);
    db.execute(&ins.expr()).await.unwrap();

    let upd = SurrealUpdate::new(Thing::new("su_content", "r1"))
        .content()
        .with_field("label", "Replaced".to_string());
    db.execute(&upd.expr()).await.unwrap();

    assert_eq!(
        read_value::<String>(&db, "su_content:r1", "label").await,
        "Replaced"
    );

    // old fields gone
    let result = db
        .execute(&surreal_expr!("SELECT VALUE name FROM ONLY su_content:r1"))
        .await
        .unwrap();
    assert!(
        result.try_get::<String>().is_none(),
        "old 'name' field should be gone after CONTENT"
    );

    cleanup(&db, &["su_content"]).await;
}

#[tokio::test]
async fn update_live_merge_partial() {
    let db = get_db().await;
    cleanup(&db, &["su_merge"]).await;

    let ins = SurrealInsert::new("su_merge")
        .with_id("r1")
        .with_field("name", "Keep".to_string())
        .with_field("score", 10i64);
    db.execute(&ins.expr()).await.unwrap();

    let upd = SurrealUpdate::new(Thing::new("su_merge", "r1"))
        .merge()
        .with_field("score", 999i64)
        .with_field("extra", "new".to_string());
    db.execute(&upd.expr()).await.unwrap();

    assert_eq!(read_value::<i64>(&db, "su_merge:r1", "score").await, 999);
    assert_eq!(
        read_value::<String>(&db, "su_merge:r1", "name").await,
        "Keep"
    );
    assert_eq!(
        read_value::<String>(&db, "su_merge:r1", "extra").await,
        "new"
    );

    cleanup(&db, &["su_merge"]).await;
}

#[tokio::test]
async fn update_live_with_thing_reference() {
    let db = get_db().await;
    cleanup(&db, &["su_parent", "su_child"]).await;

    db.execute(&surreal_expr!(
        "CREATE su_parent:p1 SET name = {}",
        "Parent"
    ))
    .await
    .unwrap();

    let ins = SurrealInsert::new("su_child")
        .with_id("c1")
        .with_field("name", "Child".to_string());
    db.execute(&ins.expr()).await.unwrap();

    let upd = SurrealUpdate::new(Thing::new("su_child", "c1"))
        .with_field("parent", Thing::new("su_parent", "p1"));
    db.execute(&upd.expr()).await.unwrap();

    assert_eq!(
        read_value::<String>(&db, "su_child:c1", "parent.name").await,
        "Parent"
    );

    cleanup(&db, &["su_parent", "su_child"]).await;
}

#[tokio::test]
async fn update_live_set_record() {
    let db = get_db().await;
    cleanup(&db, &["su_rec"]).await;

    let ins = SurrealInsert::new("su_rec")
        .with_id("r1")
        .with_field("a", 1i64);
    db.execute(&ins.expr()).await.unwrap();

    let mut record = vantage_types::Record::new();
    record.insert("a".to_string(), AnySurrealType::new(100i64));
    record.insert("b".to_string(), AnySurrealType::new("added".to_string()));

    let upd = SurrealUpdate::new(Thing::new("su_rec", "r1")).with_record(&record);
    db.execute(&upd.expr()).await.unwrap();

    assert_eq!(read_value::<i64>(&db, "su_rec:r1", "a").await, 100);
    assert_eq!(read_value::<String>(&db, "su_rec:r1", "b").await, "added");

    cleanup(&db, &["su_rec"]).await;
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealDelete — query rendering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn delete_table() {
    let del = SurrealDelete::table("users");
    assert_eq!(del.preview(), "DELETE users");
}

#[test]
fn delete_record() {
    let del = SurrealDelete::new(Thing::new("users", "john"));
    assert_eq!(del.preview(), "DELETE users:john");
}

#[test]
fn delete_with_single_condition() {
    let del = SurrealDelete::table("logs").with_condition(surreal_expr!("level = {}", "debug"));
    assert_eq!(del.preview(), "DELETE logs WHERE level = \"debug\"");
}

#[test]
fn delete_with_multiple_conditions() {
    let del = SurrealDelete::table("logs")
        .with_condition(surreal_expr!("level = {}", "debug"))
        .with_condition(surreal_expr!("age > {}", 30i64));
    assert_eq!(
        del.preview(),
        "DELETE logs WHERE level = \"debug\" AND age > 30"
    );
}

#[test]
fn delete_reserved_keyword_escaping() {
    let del = SurrealDelete::table("SELECT");
    assert_eq!(del.preview(), "DELETE ⟨SELECT⟩");
}

#[test]
fn delete_parameterized_expression() {
    let del = SurrealDelete::table("t").with_condition(surreal_expr!("x < {}", 5i64));
    let expr = del.expr();
    assert!(expr.template.contains("{}"));
}

#[test]
fn delete_with_expression_target() {
    let del = SurrealDelete::new(surreal_expr!("user WHERE active = false"));
    assert_eq!(del.preview(), "DELETE user WHERE active = false");
}

// ═══════════════════════════════════════════════════════════════════════
// SurrealDelete — live execution
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn delete_live_single_record() {
    let db = get_db().await;
    cleanup(&db, &["sd_single"]).await;

    for id in ["a", "b", "c"] {
        let ins = SurrealInsert::new("sd_single")
            .with_id(id)
            .with_field("x", 1i64);
        db.execute(&ins.expr()).await.unwrap();
    }
    assert_eq!(count_records(&db, "sd_single").await, 3);

    let del = SurrealDelete::new(Thing::new("sd_single", "b"));
    db.execute(&del.expr()).await.unwrap();

    assert_eq!(count_records(&db, "sd_single").await, 2);

    // Verify correct record removed
    let result = db
        .execute(&surreal_expr!("SELECT VALUE id FROM sd_single ORDER BY id"))
        .await
        .unwrap();
    let ids: Vec<Thing> = result.try_get().unwrap();
    let id_strs: Vec<String> = ids.iter().map(|t| t.id().to_string()).collect();
    assert!(id_strs.contains(&"a".to_string()));
    assert!(!id_strs.contains(&"b".to_string()));
    assert!(id_strs.contains(&"c".to_string()));

    cleanup(&db, &["sd_single"]).await;
}

#[tokio::test]
async fn delete_live_whole_table() {
    let db = get_db().await;
    cleanup(&db, &["sd_all"]).await;

    for i in 0..5 {
        let ins = SurrealInsert::new("sd_all")
            .with_id(format!("r{}", i))
            .with_field("v", i as i64);
        db.execute(&ins.expr()).await.unwrap();
    }
    assert_eq!(count_records(&db, "sd_all").await, 5);

    let del = SurrealDelete::table("sd_all");
    db.execute(&del.expr()).await.unwrap();

    assert_eq!(count_records(&db, "sd_all").await, 0);
}

#[tokio::test]
async fn delete_live_with_condition() {
    let db = get_db().await;
    cleanup(&db, &["sd_cond"]).await;

    for (id, score) in [("a", 10i64), ("b", 50), ("c", 90)] {
        let ins = SurrealInsert::new("sd_cond")
            .with_id(id)
            .with_field("score", score);
        db.execute(&ins.expr()).await.unwrap();
    }

    // Delete records with score < 60
    let del = SurrealDelete::table("sd_cond").with_condition(surreal_expr!("score < {}", 60i64));
    db.execute(&del.expr()).await.unwrap();

    assert_eq!(count_records(&db, "sd_cond").await, 1);
    assert_eq!(read_value::<i64>(&db, "sd_cond:c", "score").await, 90);

    cleanup(&db, &["sd_cond"]).await;
}

#[tokio::test]
async fn delete_live_nonexistent_is_ok() {
    let db = get_db().await;
    // Ensure the table exists first so SurrealDB strict mode doesn't reject it
    let ins = SurrealInsert::new("sd_ghost")
        .with_id("tmp")
        .with_field("x", 1i64);
    db.execute(&ins.expr()).await.unwrap();

    // Now deleting a nonexistent record in an existing table should not error
    let del = SurrealDelete::new(Thing::new("sd_ghost", "nope"));
    db.execute(&del.expr())
        .await
        .expect("delete of nonexistent should not error");

    cleanup(&db, &["sd_ghost"]).await;
}

// ═══════════════════════════════════════════════════════════════════════
// Full CRUD lifecycle
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn full_crud_lifecycle() {
    let db = get_db().await;
    cleanup(&db, &["crud"]).await;

    // CREATE
    let ins = SurrealInsert::new("crud")
        .with_id("item1")
        .with_field("name", "Widget".to_string())
        .with_field("price", 25i64)
        .with_field("active", true);
    db.execute(&ins.expr()).await.unwrap();

    // READ
    assert_eq!(
        read_value::<String>(&db, "crud:item1", "name").await,
        "Widget"
    );
    assert_eq!(read_value::<i64>(&db, "crud:item1", "price").await, 25);
    assert!(read_value::<bool>(&db, "crud:item1", "active").await);

    // UPDATE (SET — partial)
    let upd = SurrealUpdate::new(Thing::new("crud", "item1")).with_field("price", 30i64);
    db.execute(&upd.expr()).await.unwrap();

    assert_eq!(read_value::<i64>(&db, "crud:item1", "price").await, 30);
    assert_eq!(
        read_value::<String>(&db, "crud:item1", "name").await,
        "Widget"
    ); // preserved

    // UPDATE (MERGE — add field)
    let upd = SurrealUpdate::new(Thing::new("crud", "item1"))
        .merge()
        .with_field("category", "gadgets".to_string());
    db.execute(&upd.expr()).await.unwrap();

    assert_eq!(
        read_value::<String>(&db, "crud:item1", "category").await,
        "gadgets"
    );
    assert_eq!(read_value::<i64>(&db, "crud:item1", "price").await, 30); // preserved

    // UPDATE (CONTENT — replace all)
    let upd = SurrealUpdate::new(Thing::new("crud", "item1"))
        .content()
        .with_field("replacement", true);
    db.execute(&upd.expr()).await.unwrap();

    assert!(read_value::<bool>(&db, "crud:item1", "replacement").await);
    let old_name = db
        .execute(&surreal_expr!("SELECT VALUE name FROM ONLY crud:item1"))
        .await
        .unwrap();
    assert!(
        old_name.try_get::<String>().is_none(),
        "old fields should be gone after CONTENT"
    );

    // DELETE
    let del = SurrealDelete::new(Thing::new("crud", "item1"));
    db.execute(&del.expr()).await.unwrap();

    assert_eq!(count_records(&db, "crud").await, 0);
}

// ═══════════════════════════════════════════════════════════════════════
// Cross-statement: insert → select with conditions → update → verify
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn insert_select_update_flow() {
    let db = get_db().await;
    cleanup(&db, &["flow"]).await;

    // Insert several records
    for (id, name, score) in [
        ("alice", "Alice", 85i64),
        ("bob", "Bob", 45),
        ("carol", "Carol", 72),
        ("dave", "Dave", 33),
    ] {
        let ins = SurrealInsert::new("flow")
            .with_id(id)
            .with_field("name", name.to_string())
            .with_field("score", score)
            .with_field("passed", false);
        db.execute(&ins.expr()).await.unwrap();
    }

    // Select passing scores (>= 50), ordered descending
    // Use row select (not VALUE) since SurrealDB requires ORDER BY field in selection
    let mut select = SurrealSelect::new();
    select.set_source("flow", None);
    select.add_field("name".to_string());
    select.add_field("score".to_string());
    select.add_where_condition(surreal_expr!("score >= {}", 50i64));
    select.add_order_by(surreal_expr!("score"), false);

    let result = db.execute(&select.expr()).await.unwrap();
    let rows: Vec<indexmap::IndexMap<String, AnySurrealType>> = result.try_get().unwrap();
    let names: Vec<String> = rows
        .iter()
        .map(|r| r.get("name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert_eq!(names, vec!["Alice", "Carol"]);

    // Update all passing students
    let query = surreal_expr!("UPDATE flow SET passed = {} WHERE score >= {}", true, 50i64);
    db.execute(&query).await.unwrap();

    assert!(read_value::<bool>(&db, "flow:alice", "passed").await);
    assert!(!read_value::<bool>(&db, "flow:bob", "passed").await);
    assert!(read_value::<bool>(&db, "flow:carol", "passed").await);
    assert!(!read_value::<bool>(&db, "flow:dave", "passed").await);

    // Delete failing students
    let del = SurrealDelete::table("flow").with_condition(surreal_expr!("passed = {}", false));
    db.execute(&del.expr()).await.unwrap();

    assert_eq!(count_records(&db, "flow").await, 2);

    // Verify only passing remain
    let result = db
        .execute(&surreal_expr!("SELECT VALUE name FROM flow"))
        .await
        .unwrap();
    let remaining: Vec<String> = result.try_get().unwrap();
    assert_eq!(remaining.len(), 2);
    assert!(remaining.contains(&"Alice".to_string()));
    assert!(remaining.contains(&"Carol".to_string()));

    cleanup(&db, &["flow"]).await;
}

// ═══════════════════════════════════════════════════════════════════════
// Relationship traversal with INSERT + UPDATE + SELECT
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn relationship_traversal() {
    let db = get_db().await;
    cleanup(&db, &["rel_bakery", "rel_product"]).await;

    // Create bakery
    let ins = SurrealInsert::new("rel_bakery")
        .with_id("hv")
        .with_field("name", "Hill Valley Bakery".to_string());
    db.execute(&ins.expr()).await.unwrap();

    // Create products referencing bakery
    for (id, name, price) in [
        ("cupcake", "Flux Cupcake", 5i64),
        ("bread", "Time Bread", 3),
        ("pie", "Paradox Pie", 8),
    ] {
        let ins = SurrealInsert::new("rel_product")
            .with_id(id)
            .with_field("name", name.to_string())
            .with_field("price", price)
            .with_field("bakery", Thing::new("rel_bakery", "hv"));
        db.execute(&ins.expr()).await.unwrap();
    }

    // Select product names with bakery name via traversal
    let result = db
        .execute(&surreal_expr!(
            "SELECT name, bakery.name AS bakery_name FROM rel_product ORDER BY name"
        ))
        .await
        .unwrap();

    let rows: Vec<indexmap::IndexMap<String, AnySurrealType>> = result.try_get().unwrap();
    assert_eq!(rows.len(), 3);
    let names: Vec<String> = rows
        .iter()
        .map(|r| r.get("name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert_eq!(names, vec!["Flux Cupcake", "Paradox Pie", "Time Bread"]);
    let bakery_names: Vec<String> = rows
        .iter()
        .map(|r| r.get("bakery_name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert!(bakery_names.iter().all(|n| n == "Hill Valley Bakery"));

    // Update bakery name
    let upd = SurrealUpdate::new(Thing::new("rel_bakery", "hv"))
        .with_field("name", "Hill Valley Bakery 2.0".to_string());
    db.execute(&upd.expr()).await.unwrap();

    // Verify traversal reflects update
    assert_eq!(
        read_value::<String>(&db, "rel_product:cupcake", "bakery.name").await,
        "Hill Valley Bakery 2.0"
    );

    // Delete bakery — products keep dangling ref
    let del = SurrealDelete::new(Thing::new("rel_bakery", "hv"));
    db.execute(&del.expr()).await.unwrap();

    assert_eq!(count_records(&db, "rel_bakery").await, 0);
    // Products still exist
    assert_eq!(count_records(&db, "rel_product").await, 3);

    cleanup(&db, &["rel_bakery", "rel_product"]).await;
}

// ═══════════════════════════════════════════════════════════════════════
// Edge cases
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn insert_field_order_preserved() {
    // IndexMap should preserve insertion order
    let ins = SurrealInsert::new("t")
        .with_field("z_last", 1i64)
        .with_field("a_first", 2i64)
        .with_field("m_mid", 3i64);

    let p = ins.preview();
    let z_pos = p.find("z_last").unwrap();
    let a_pos = p.find("a_first").unwrap();
    let m_pos = p.find("m_mid").unwrap();
    assert!(z_pos < a_pos, "field order not preserved");
    assert!(a_pos < m_pos, "field order not preserved");
}

#[test]
fn update_field_order_preserved() {
    let upd = SurrealUpdate::new(Thing::new("t", "1"))
        .with_field("z", 1i64)
        .with_field("a", 2i64)
        .with_field("m", 3i64);

    let p = upd.preview();
    let z_pos = p.find("z =").unwrap();
    let a_pos = p.find("a =").unwrap();
    let m_pos = p.find("m =").unwrap();
    assert!(z_pos < a_pos);
    assert!(a_pos < m_pos);
}

#[test]
fn thing_accessors() {
    let t = Thing::new("users", "alice");
    assert_eq!(t.table(), "users");
    assert_eq!(t.id(), "alice");
    assert_eq!(t.to_string(), "users:alice");
}

#[test]
fn thing_display() {
    let t = Thing::new("order", "123");
    assert_eq!(format!("{}", t), "order:123");
}

#[test]
fn thing_parse() {
    let t: Thing = "order:456".parse().unwrap();
    assert_eq!(t.table(), "order");
    assert_eq!(t.id(), "456");
}

#[test]
fn thing_expressive() {
    let t = Thing::new("bakery", "hv");
    let expr = t.expr();
    assert_eq!(expr.preview(), "bakery:hv");
}
