//! Roll-up columns: `<~` over a `REFERENCE` link, a correlated `$parent`
//! subquery (and a warning) over a plain one, with the same numbers.
//!
//! Each test seeds its own database:
//! - invoice:one has lines a (net 10), b (net 6, void), d (no net) and x (net 2);
//! - invoice:two has line c (net 7); line x also links it, through `credit`;
//! - invoice:three has no lines.

use std::io::Write;
use std::sync::{Arc, Mutex};

use serde_json::{Value as JsonValue, json};
use surreal_client::SurrealConnection;
use uuid::Uuid;
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::ExprDataSource;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_table::table::{RollupOp, Table};
use vantage_types::EmptyEntity;

fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

/// A fresh database with the seed, its links declared `REFERENCE` or not,
/// and the schema checked.
async fn seeded(reference: bool) -> SurrealDB {
    let database = format!("rollup_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    let reference = if reference { " REFERENCE" } else { "" };
    let schema = format!(
        "DEFINE TABLE invoice SCHEMALESS; DEFINE TABLE line SCHEMALESS; \
         DEFINE FIELD invoice ON line TYPE option<record<invoice>>{reference}; \
         DEFINE FIELD credit ON line TYPE option<record<invoice>>{reference}; \
         CREATE invoice:one, invoice:two, invoice:three; \
         CREATE line:a SET invoice = invoice:one, net = 10, void = false; \
         CREATE line:b SET invoice = invoice:one, net = 6, void = true; \
         CREATE line:d SET invoice = invoice:one, void = false; \
         CREATE line:x SET invoice = invoice:one, credit = invoice:two, net = 2, void = false; \
         CREATE line:c SET invoice = invoice:two, net = 7, void = false;"
    );
    for statement in schema.split("; ") {
        db.execute(&surreal_expr!(statement)).await.unwrap();
    }
    db.check_references().await.unwrap();
    db
}

fn line_table(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
    Table::new("line", db)
        .with_id_column("id")
        .with_column_of::<i64>("net")
        .with_column_of::<bool>("void")
}

/// Every invoice with one roll-up column per op over its lines, the
/// non-void ones when `only_live`.
fn invoices(db: SurrealDB, only_live: bool) -> Table<SurrealDB, EmptyEntity> {
    let ops = [
        ("lines", RollupOp::Count, None),
        ("total", RollupOp::Sum, Some("net")),
        ("cheapest", RollupOp::Min, Some("net")),
        ("dearest", RollupOp::Max, Some("net")),
        ("average", RollupOp::Avg, Some("net")),
    ];
    let invoices = Table::new("invoice", db)
        .with_id_column("id")
        .with_many("lines", "invoice", line_table);
    ops.into_iter()
        .fold(invoices, |invoices, (name, op, column)| {
            let condition = only_live.then(|| surreal_expr!("void = {}", false));
            invoices
                .with_rollup(name, "lines", op, column, condition)
                .unwrap()
        })
}

/// Each invoice's roll-ups, by id.
async fn rollups(invoices: &Table<SurrealDB, EmptyEntity>) -> JsonValue {
    let rows = invoices.list_values().await.unwrap();
    let mut out = serde_json::Map::new();
    for (id, row) in rows {
        let get = |name: &str| {
            row.get(name)
                .cloned()
                .map_or(JsonValue::Null, JsonValue::from)
        };
        out.insert(
            id.to_string(),
            json!([
                get("lines"),
                get("total"),
                get("cheapest"),
                get("dearest"),
                get("average")
            ]),
        );
    }
    JsonValue::Object(out)
}

/// `[count, sum, min, max, avg]` per invoice, worked out by hand.
fn expected(only_live: bool) -> JsonValue {
    let first = if only_live {
        json!([3, 12, 2, 10, 6.0])
    } else {
        json!([4, 18, 2, 10, 6.0])
    };
    json!({
        "invoice:one": first,
        "invoice:two": [1, 7, 7, 7, 7.0],
        "invoice:three": [0, 0, null, null, null],
    })
}

/// The warnings logged while `build` runs.
fn warnings_during<R>(build: impl FnOnce() -> R) -> (R, String) {
    #[derive(Clone)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);
    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buffer = Buffer(Arc::default());
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .finish();
    let built = tracing::subscriber::with_default(subscriber, build);
    let logged = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    (built, logged)
}

#[tokio::test]
async fn rollups_over_a_reference_link_read_through_record_references() {
    let db = seeded(true).await;
    for only_live in [false, true] {
        let (table, warnings) = warnings_during(|| invoices(db.clone(), only_live));
        assert_eq!(warnings, "");
        assert_eq!(
            rollups(&table).await,
            expected(only_live),
            "only_live={only_live}"
        );
    }

    let total = Table::<SurrealDB, EmptyEntity>::new("invoice", db.clone())
        .with_many("lines", "invoice", line_table)
        .with_rollup(
            "total",
            "lines",
            RollupOp::Sum,
            Some("net"),
            Some(surreal_expr!("void = {}", false)),
        )
        .unwrap();
    assert_eq!(
        total.select().preview(),
        "SELECT (math::sum(<~(line FIELD invoice)[WHERE void = false AND net != NONE \
         AND net != NULL].net)) AS total FROM invoice"
    );
}

#[tokio::test]
async fn rollups_over_a_plain_link_use_parent_and_warn() {
    let db = seeded(false).await;
    for only_live in [false, true] {
        let (table, warnings) = warnings_during(|| invoices(db.clone(), only_live));
        assert_eq!(
            rollups(&table).await,
            expected(only_live),
            "only_live={only_live}"
        );
        // Once per link: the first build warns, the second doesn't.
        assert_eq!(
            warnings.contains("`DEFINE FIELD invoice ON line TYPE record<invoice> REFERENCE;`"),
            !only_live,
            "{warnings}"
        );
    }

    let table = Table::<SurrealDB, EmptyEntity>::new("invoice", db)
        .with_many("lines", "invoice", line_table)
        .with_rollup("lines", "lines", RollupOp::Count, None, None)
        .unwrap();
    assert_eq!(
        table.select().preview(),
        "SELECT (count((SELECT VALUE id FROM line WHERE invoice = $parent.id))) AS lines \
         FROM invoice"
    );
}
