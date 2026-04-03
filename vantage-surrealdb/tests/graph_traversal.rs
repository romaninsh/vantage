//! Live graph traversal tests against the v1 bakery database.
//!
//! Requires: `cd scripts && ./ingress.sh` to populate the v1 database.
//! These tests are read-only — they don't modify the v1 data.

use vantage_expressions::{ExprDataSource, Expressive, Selectable};
use vantage_surrealdb::{
    operation::RefOperation, select::SurrealSelect, surreal_expr, surrealdb::SurrealDB,
    thing::Thing, types::AnySurrealType,
};

async fn v1_db() -> SurrealDB {
    let dsn = "cbor://root:root@localhost:8000/bakery/v1";
    let client = surreal_client::SurrealConnection::dsn(dsn)
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB — is it running?");
    SurrealDB::new(client)
}

// ── Query 01: bakery->owns->product (graph traversal) ───────────────────

#[tokio::test]
async fn query01_bakery_products_via_graph() {
    let db = v1_db().await;

    // Graph traversal — ORDER BY is ignored by SurrealDB on graph sources
    let inner = SurrealSelect::new()
        .from(Thing::new("bakery", "hill_valley").rref("owns", "product"))
        .with_where(surreal_expr!("is_deleted = {}", false));

    let rows = inner.get(&db).await.unwrap();
    assert_eq!(rows.len(), 5);

    // Wrap in subquery to get ORDER BY working
    let mut outer = SurrealSelect::new();
    outer.set_source(inner.expr(), None);
    outer.add_order_by(surreal_expr!("name"), true);

    let rows = outer.get(&db).await.unwrap();
    assert_eq!(rows.len(), 5);

    let names: Vec<String> = rows
        .iter()
        .map(|r| r.get("name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert_eq!(names[0], "DeLorean Doughnut");
    assert_eq!(names[4], "Time Traveler Tart");
}

// ── Query 02: bakery<-belongs_to<-client (reverse traversal) ────────────

#[tokio::test]
async fn query02_bakery_clients_via_reverse_graph() {
    let db = v1_db().await;

    let inner =
        SurrealSelect::new().from(Thing::new("bakery", "hill_valley").lref("belongs_to", "client"));

    // Wrap to get ordered results
    let mut outer = SurrealSelect::new();
    outer.set_source(inner.expr(), None);
    outer.add_order_by(surreal_expr!("name"), true);

    let rows = outer.get(&db).await.unwrap();
    assert_eq!(rows.len(), 3);

    let names: Vec<String> = rows
        .iter()
        .map(|r| r.get("name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert_eq!(names, vec!["Biff Tannen", "Doc Brown", "Marty McFly"]);
}

// ── Query 03: product fields with embedded doc access ───────────────────

#[tokio::test]
async fn query03_product_stock() {
    let db = v1_db().await;

    let select = SurrealSelect::new()
        .from("product")
        .field("name")
        .field("price")
        .with_expression(surreal_expr!("inventory.stock"), Some("stock".into()))
        .with_where(surreal_expr!("is_deleted = {}", false))
        .with_order_by("name", true);

    let rows = select.get(&db).await.unwrap();
    assert_eq!(rows.len(), 5);

    // Check first product has stock
    let stock = rows[0].get("stock").unwrap().try_get::<i64>().unwrap();
    assert!(stock > 0);
}

// ── Query 05: client->placed->order (multi-hop) ────────────────────────

#[tokio::test]
async fn query05_client_orders() {
    let db = v1_db().await;

    let select = SurrealSelect::new().from(Thing::new("client", "marty").rref("placed", "order"));

    let rows = select.get(&db).await.unwrap();
    assert_eq!(rows.len(), 1); // Marty has 1 order
}

#[tokio::test]
async fn query05_doc_orders() {
    let db = v1_db().await;

    let select = SurrealSelect::new().from(Thing::new("client", "doc").rref("placed", "order"));

    let rows = select.get(&db).await.unwrap();
    assert_eq!(rows.len(), 2); // Doc has 2 orders
}

// ── Query 06: same as 01 (graph products, non-deleted) ──────────────────

#[tokio::test]
async fn query06_bakery_products_not_deleted() {
    let db = v1_db().await;

    let select = SurrealSelect::new()
        .from(Thing::new("bakery", "hill_valley").rref("owns", "product"))
        .with_where(surreal_expr!("is_deleted = {}", false));

    let rows = select.get(&db).await.unwrap();
    assert_eq!(rows.len(), 5);
}

// ── Query 10: reverse graph in WHERE clause ─────────────────────────────

#[tokio::test]
async fn query10_low_stock_products() {
    let db = v1_db().await;

    let select = SurrealSelect::new()
        .from("product")
        .field("name")
        .with_expression(surreal_expr!("inventory.stock"), None)
        .with_where(
            Thing::new("bakery", "hill_valley").in_(surreal_expr!("").lref("owns", "bakery")),
        )
        .with_where(surreal_expr!("inventory.stock < {}", 20i64))
        .with_where(surreal_expr!("is_deleted = {}", false))
        .with_order_by("name", true);

    let rows = select.get(&db).await.unwrap();

    // sea_pie (15) and time_tart (20 — not < 20) => only sea_pie
    assert!(rows.len() >= 1);
    let names: Vec<String> = rows
        .iter()
        .map(|r| r.get("name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert!(names.contains(&"Enchantment Under the Sea Pie".to_string()));
}

// ── Subquery as source ──────────────────────────────────────────────────

#[tokio::test]
async fn subquery_paying_clients() {
    let db = v1_db().await;

    // Inner: get paying client IDs via graph
    let inner = SurrealSelect::new()
        .from(Thing::new("bakery", "hill_valley").lref("belongs_to", "client"))
        .field("name")
        .with_where(surreal_expr!("is_paying_client = {}", true))
        .with_order_by("name", true);

    let rows = inner.get(&db).await.unwrap();
    assert_eq!(rows.len(), 2); // Marty + Doc are paying

    let names: Vec<String> = rows
        .iter()
        .map(|r| r.get("name").unwrap().try_get::<String>().unwrap())
        .collect();
    assert_eq!(names, vec!["Doc Brown", "Marty McFly"]);
}

// ── Aggregation over graph traversal ────────────────────────────────────

#[tokio::test]
async fn aggregate_product_count_via_graph() {
    let db = v1_db().await;

    // count products owned by hill_valley bakery
    let select = SurrealSelect::new()
        .from(Thing::new("bakery", "hill_valley").rref("owns", "product"))
        .with_where(surreal_expr!("is_deleted = {}", false));

    let count_query = select.as_count();
    let result: AnySurrealType = db.execute(&count_query.expr()).await.unwrap();
    let count = result.try_get::<i64>().unwrap();
    assert_eq!(count, 5);
}

// ── Verify query rendering matches v1.surql patterns ────────────────────

#[test]
fn render_query01_wrapped() {
    let inner = SurrealSelect::new()
        .from(Thing::new("bakery", "hill_valley").rref("owns", "product"))
        .with_where(surreal_expr!("is_deleted = {}", false));

    let mut outer = SurrealSelect::new();
    outer.set_source(inner.expr(), None);
    outer.add_order_by(surreal_expr!("name"), true);

    assert_eq!(
        outer.preview(),
        "SELECT * FROM (SELECT * FROM bakery:hill_valley->owns->product WHERE is_deleted = false) ORDER BY name"
    );
}

#[test]
fn render_query02_wrapped() {
    let inner =
        SurrealSelect::new().from(Thing::new("bakery", "hill_valley").lref("belongs_to", "client"));

    let mut outer = SurrealSelect::new();
    outer.set_source(inner.expr(), None);
    outer.add_order_by(surreal_expr!("name"), true);

    assert_eq!(
        outer.preview(),
        "SELECT * FROM (SELECT * FROM bakery:hill_valley<-belongs_to<-client) ORDER BY name"
    );
}

#[test]
fn render_query05() {
    let select = SurrealSelect::new().from(Thing::new("client", "marty").rref("placed", "order"));

    assert_eq!(
        select.preview(),
        "SELECT * FROM client:marty->placed->order"
    );
}

#[test]
fn render_query10() {
    let select = SurrealSelect::new()
        .from("product")
        .field("name")
        .with_expression(surreal_expr!("inventory.stock"), None)
        .with_where(
            Thing::new("bakery", "hill_valley").in_(surreal_expr!("").lref("owns", "bakery")),
        )
        .with_where(surreal_expr!("inventory.stock < {}", 20i64))
        .with_where(surreal_expr!("is_deleted = {}", false));

    assert_eq!(
        select.preview(),
        "SELECT name, inventory.stock FROM product WHERE bakery:hill_valley IN <-owns<-bakery AND inventory.stock < 20 AND is_deleted = false"
    );
}
