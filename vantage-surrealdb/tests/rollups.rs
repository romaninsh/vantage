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
use vantage_expressions::traits::selectable::Selectable;
use vantage_surrealdb::SurrealSelect;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::vista::spec::SurrealVistaSpec;
use vantage_table::pagination::Pagination;
use vantage_table::sorting::SortDirection;
use vantage_table::table::{Aggregate, RelationQuery, RollupOp, Table};
use vantage_types::EmptyEntity;
use vantage_vista::SpecResolver;

fn surreal_dsn(database: &str) -> String {
    let base = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "cbor://root:root@localhost:8000/bakery/v2".to_string());
    let (prefix, _) = base.rsplit_once('/').unwrap_or((&base, ""));
    format!("{}/{}", prefix, database)
}

/// A fresh database with the seed, its links declared `REFERENCE` or not,
/// and the schema checked.
async fn seeded(reference: bool) -> SurrealDB {
    seeded_with(reference, reference).await
}

/// [`seeded`] with `line.invoice` and `line.credit` each declared
/// `REFERENCE` or not.
async fn seeded_with(invoice: bool, credit: bool) -> SurrealDB {
    let database = format!("rollup_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    let reference = |on: bool| if on { " REFERENCE" } else { "" };
    let (invoice, credit) = (reference(invoice), reference(credit));
    let schema = format!(
        "DEFINE TABLE invoice SCHEMALESS; DEFINE TABLE line SCHEMALESS; \
         DEFINE FIELD invoice ON line TYPE option<record<invoice>>{invoice}; \
         DEFINE FIELD credit ON line TYPE option<record<invoice>>{credit}; \
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
    db.check_schema().await.unwrap();
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
    // `line` holds two `REFERENCE` fields, so `<~line` would mix them: the
    // roll-up reads through `$parent`.
    assert_eq!(
        total.select().preview(),
        "SELECT (math::sum((SELECT VALUE net FROM line WHERE invoice = $parent.id \
         AND void = false AND net != NONE AND net != NULL))) AS total FROM invoice"
    );
}

#[tokio::test]
async fn rollup_over_a_single_reference_link_uses_bare_lookup() {
    let db = seeded_with(true, false).await;
    for only_live in [false, true] {
        let (table, warnings) = warnings_during(|| invoices(db.clone(), only_live));
        assert_eq!(warnings, "");
        assert_eq!(
            rollups(&table).await,
            expected(only_live),
            "only_live={only_live}"
        );
    }

    let total = Table::<SurrealDB, EmptyEntity>::new("invoice", db)
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
        "SELECT (math::sum(<~line[WHERE void = false AND net != NONE \
         AND net != NULL].net)) AS total FROM invoice"
    );
}

/// A query-sourced child aliased like the stored table keeps its own rows:
/// `<~line` would read every line, void ones included.
#[tokio::test]
async fn rollup_over_a_query_sourced_child_keeps_its_query() {
    let db = seeded(true).await;
    fn live_lines(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
        let mut select = SurrealSelect::new();
        select.add_source("line", None);
        select.add_where_condition(surreal_expr!("void = {}", false));
        Table::from_select(db, "line", select)
            .with_id_column("id")
            .with_column_of::<i64>("net")
    }
    let table = Table::<SurrealDB, EmptyEntity>::new("invoice", db)
        .with_id_column("id")
        .with_many("lines", "invoice", live_lines)
        .with_rollup("lines", "lines", RollupOp::Count, None, None)
        .unwrap();
    let counts = rollups(&table).await;
    assert_eq!(
        [&counts["invoice:one"][0], &counts["invoice:two"][0]],
        [&json!(3), &json!(1)]
    );
}

/// The error a roll-up gives when its column is read, from each way in: the
/// check a factory runs on a finished table, the select, and the read.
async fn query_time_error(table: &Table<SurrealDB, EmptyEntity>) -> String {
    let checked = table.check_expressions().unwrap_err();
    let selected = table.try_select().unwrap_err();
    let read = table.list_values().await.unwrap_err();
    assert_eq!(checked.message(), selected.message());
    assert_eq!(checked.message(), read.message());
    read.message().to_string()
}

/// A computed child column is aggregated by its expression, read through a
/// sub-select.
#[tokio::test]
async fn rollup_over_a_computed_child_column_aggregates_the_expression() {
    let db = seeded(true).await;
    fn doubled_lines(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
        line_table(db).with_expression("net", |_| surreal_expr!("(net ?? 0) * 2"))
    }
    let table = Table::<SurrealDB, EmptyEntity>::new("invoice", db)
        .with_id_column("id")
        .with_many("lines", "invoice", doubled_lines)
        .with_rollup("total", "lines", RollupOp::Sum, Some("net"), None)
        .unwrap();
    assert_eq!(
        read(&table, "total").await,
        json!({"invoice:one": 36, "invoice:two": 14, "invoice:three": 0})
    );
}

/// Column `column` of every row, by id.
async fn read(table: &Table<SurrealDB, EmptyEntity>, column: &str) -> JsonValue {
    let rows = table.list_values().await.unwrap();
    let out: serde_json::Map<String, JsonValue> = rows
        .into_iter()
        .map(|(id, row)| {
            let value = row
                .get(column)
                .cloned()
                .map_or(JsonValue::Null, JsonValue::from);
            (id.to_string(), value)
        })
        .collect();
    JsonValue::Object(out)
}

/// Invoices with `aggregate` over their lines that carry a net, as `name`.
fn with_relation(
    db: SurrealDB,
    q: RelationQuery<vantage_surrealdb::Expr>,
    aggregate: Aggregate,
) -> Table<SurrealDB, EmptyEntity> {
    Table::new("invoice", db)
        .with_id_column("id")
        .with_many("lines", "invoice", line_table)
        .with_relation_column(
            "r",
            q.with_condition(surreal_expr!("net != NONE")),
            aggregate,
        )
        .unwrap()
}

/// `table.ref(…)` in a script builds the expression the Rust API does.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn surreal_rhai_relation_chain_reads_values() {
    use vantage_surrealdb::vista::rhai_source::eval_column_expr;
    let db = seeded(true).await;
    let invoices = Table::<SurrealDB, EmptyEntity>::new("invoice", db)
        .with_id_column("id")
        .with_many("lines", "invoice", line_table);
    let expr = eval_column_expr(
        r#"table.ref("lines").where("void", false).count()"#,
        Some(invoices.clone()),
        vantage_rhai::Env::new(),
    )
    .unwrap();
    let invoices = invoices.with_expression("live", move |_| expr.clone());
    assert_eq!(
        read(&invoices, "live").await,
        json!({"invoice:one": 3, "invoice:two": 1, "invoice:three": 0})
    );
}

#[tokio::test]
async fn surreal_relation_join_json_ordered() {
    let invoices = with_relation(
        seeded(true).await,
        RelationQuery::new("lines").with_order("net", SortDirection::Ascending),
        Aggregate::JoinJson("net".into()),
    );
    assert_eq!(
        read(&invoices, "r").await,
        json!({"invoice:one": [2, 6, 10], "invoice:two": [7], "invoice:three": []})
    );
}

#[tokio::test]
async fn surreal_relation_exists() {
    let invoices = with_relation(
        seeded(true).await,
        RelationQuery::new("lines").with_condition(surreal_expr!("void = {}", true)),
        Aggregate::Exists,
    );
    assert_eq!(
        read(&invoices, "r").await,
        json!({"invoice:one": true, "invoice:two": false, "invoice:three": false})
    );
    // An `OR` stays inside the correlation: invoice:three has no lines, so
    // line c's `net = 7` (invoice:two's) mustn't count for it. Plain links
    // correlate in the sub-select's WHERE, next to the `OR`.
    let either = Table::<SurrealDB, EmptyEntity>::new("invoice", seeded(false).await)
        .with_id_column("id")
        .with_many("lines", "invoice", line_table)
        .with_relation_column(
            "r",
            RelationQuery::new("lines").with_condition(surreal_expr!("void = true OR net = 7")),
            Aggregate::Exists,
        )
        .unwrap();
    assert_eq!(
        read(&either, "r").await,
        json!({"invoice:one": true, "invoice:two": true, "invoice:three": false})
    );
}

/// A limited aggregate takes its rows first, then skips missing values, as
/// SQL does: invoice:one's lowest-net line (d) has none, so its sum is 0.
#[tokio::test]
async fn surreal_relation_limit_before_skipping_missing_values() {
    let invoices = Table::<SurrealDB, EmptyEntity>::new("invoice", seeded(true).await)
        .with_id_column("id")
        .with_many("lines", "invoice", line_table)
        .with_relation_column(
            "r",
            RelationQuery::new("lines")
                .with_order("net", SortDirection::Ascending)
                .with_limit(1),
            Aggregate::Sum("net".into()),
        )
        .unwrap();
    assert_eq!(
        read(&invoices, "r").await,
        json!({"invoice:one": 0, "invoice:two": 7, "invoice:three": 0})
    );
}

#[tokio::test]
async fn surreal_relation_first() {
    let invoices = with_relation(
        seeded(true).await,
        RelationQuery::new("lines").with_order("net", SortDirection::Descending),
        Aggregate::First("net".into()),
    );
    assert_eq!(
        read(&invoices, "r").await,
        json!({"invoice:one": 10, "invoice:two": 7, "invoice:three": null})
    );
}

/// A has_one's field keeps the target's own conditions and the chain's
/// `where`, as on SQL: a hidden invoice gives null, not its value through
/// the link.
#[tokio::test]
async fn surreal_relation_field_keeps_target_conditions() {
    let db = seeded(true).await;
    for statement in [
        "UPDATE invoice:one SET number = 1, open = true",
        "UPDATE invoice:two SET number = 2, open = false",
    ] {
        db.execute(&surreal_expr!(statement)).await.unwrap();
    }
    fn open_invoices(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
        Table::new("invoice", db)
            .with_id_column("id")
            .with_column_of::<i64>("number")
            .with_condition(surreal_expr!("open = {}", true))
    }
    let lines = |q: RelationQuery<vantage_surrealdb::Expr>| {
        line_table(db.clone())
            .with_one("invoice", "invoice", open_invoices)
            .with_relation_column("number", q, Aggregate::Field("number".into()))
            .unwrap()
    };
    assert_eq!(
        read(&lines(RelationQuery::new("invoice")), "number").await,
        json!({"line:a": 1, "line:b": 1, "line:c": null, "line:d": 1, "line:x": 1})
    );
    let not_one = RelationQuery::new("invoice").with_condition(surreal_expr!("number != {}", 1));
    assert_eq!(
        read(&lines(not_one), "number").await,
        json!({"line:a": null, "line:b": null, "line:c": null, "line:d": null, "line:x": null})
    );
}

/// A has_many target with a window has no clear set to aggregate: refused
/// when the column is read.
#[tokio::test]
async fn rollup_over_a_paginated_child_is_refused() {
    let db = seeded(true).await;
    fn first_line(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
        let mut lines = line_table(db);
        lines.set_pagination(Some(Pagination::new(2, 1)));
        lines
    }
    let table = Table::<SurrealDB, EmptyEntity>::new("invoice", db)
        .with_many("lines", "invoice", first_line)
        .with_rollup("lines", "lines", RollupOp::Count, None, None)
        .unwrap();
    assert_eq!(
        query_time_error(&table).await,
        "a roll-up's relation target can't have pagination"
    );
}

/// `node.with_many("children", "parent", node)`: `$parent` names the outer
/// row, so the subquery over the node's own table needs no alias. A column
/// aggregating itself is the named error, not an endless build.
#[tokio::test]
async fn a_self_referencing_rollup_counts_each_rows_children() {
    let db = seeded(true).await;
    for statement in [
        "DEFINE TABLE node SCHEMALESS",
        "CREATE node:root SET weight = 1",
        "CREATE node:a SET parent = node:root, weight = 2",
        "CREATE node:b SET parent = node:root, weight = 3",
        "CREATE node:c SET parent = node:a, weight = 4",
    ] {
        db.execute(&surreal_expr!(statement)).await.unwrap();
    }
    fn node(db: SurrealDB) -> Table<SurrealDB, EmptyEntity> {
        Table::new("node", db)
            .with_id_column("id")
            .with_column_of::<i64>("weight")
            .with_many("children", "parent", node)
    }
    let nodes = node(db.clone())
        .with_rollup("carried", "children", RollupOp::Sum, Some("weight"), None)
        .unwrap();
    let rows = nodes.list_values().await.unwrap();
    let carried: serde_json::Map<String, JsonValue> = rows
        .into_iter()
        .map(|(id, row)| (id.to_string(), JsonValue::from(row["carried"].clone())))
        .collect();
    assert_eq!(
        JsonValue::Object(carried),
        json!({"node:root": 5, "node:a": 4, "node:b": 0, "node:c": 0})
    );

    let itself = node(db)
        .with_rollup("weight", "children", RollupOp::Sum, Some("weight"), None)
        .unwrap();
    assert_eq!(
        query_time_error(&itself).await,
        "a roll-up column can't aggregate itself"
    );
}

/// Invoices and lines both keyed by a `doc`, not linked by the invoice id:
/// invoice:one (doc:a) has lines a (vat 5) and b (vat 7), invoice:two (doc:b)
/// line c (vat 3), invoice:three has no doc.
async fn seeded_by_doc(reference: bool) -> SurrealDB {
    let database = format!("rollup_{}", Uuid::new_v4().simple());
    let client = SurrealConnection::dsn(surreal_dsn(&database))
        .expect("Invalid SURREALDB_URL DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    let db = SurrealDB::new(client);
    let reference = if reference { " REFERENCE" } else { "" };
    let schema = format!(
        "DEFINE TABLE doc SCHEMALESS; DEFINE TABLE invoice SCHEMALESS; \
         DEFINE TABLE line SCHEMALESS; \
         DEFINE FIELD doc ON line TYPE option<record<doc>>{reference}; \
         CREATE doc:a, doc:b; \
         CREATE invoice:one SET doc = doc:a; CREATE invoice:two SET doc = doc:b; \
         CREATE invoice:three; \
         CREATE line:a SET doc = doc:a, vat = 5; CREATE line:b SET doc = doc:a, vat = 7; \
         CREATE line:c SET doc = doc:b, vat = 3;"
    );
    for statement in schema.split("; ") {
        db.execute(&surreal_expr!(statement)).await.unwrap();
    }
    db.check_schema().await.unwrap();
    db
}

/// `invoice` built from a spec whose `lines` relation is joined by `keys`.
fn keyed_invoices(db: &SurrealDB, keys: &str) -> Table<SurrealDB, EmptyEntity> {
    let spec = |yaml: &str| -> SurrealVistaSpec { serde_yaml_ng::from_str(yaml).unwrap() };
    let line = spec(
        "name: line\n\
         columns:\n  id: { type: thing, flags: [id] }\n  doc: { type: string }\n  \
         vat: { type: int }\n",
    );
    let invoice = spec(&format!(
        "name: invoice\n\
         columns:\n  id: {{ type: thing, flags: [id] }}\n  doc: {{ type: string, references: doc }}\n\
         references:\n  lines: {{ table: line, kind: has_many, keys: {keys} }}\n"
    ));
    let resolver: Arc<dyn SpecResolver<SurrealVistaSpec>> =
        Arc::new(move |name: &str| (name == "line").then(|| line.clone()));
    db.vista_factory()
        .with_resolver(resolver)
        .table_from_spec(&invoice)
        .unwrap()
}

/// A `keys:` relation joins the child's key column to the parent's own key
/// column, not the relation name to the parent's id.
#[tokio::test]
async fn rollup_over_a_keyed_relation_joins_on_its_keys() {
    let keys = "[{ from: doc, to: doc }]";
    for reference in [true, false] {
        let db = seeded_by_doc(reference).await;
        let table = keyed_invoices(&db, keys)
            .with_rollup("vat", "lines", RollupOp::Sum, Some("vat"), None)
            .unwrap();
        // The roll-up is rendered, and the slow form warned about, when the
        // query is built.
        let (select, warnings) = warnings_during(|| table.try_select().unwrap().preview());
        let rows = table.list_values().await.unwrap();
        let mut sums: Vec<(String, JsonValue)> = rows
            .into_iter()
            .map(|(id, row)| {
                let vat = row
                    .get("vat")
                    .cloned()
                    .map_or(JsonValue::Null, JsonValue::from);
                (id.to_string(), vat)
            })
            .collect();
        sums.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            sums,
            [
                ("invoice:one".to_string(), json!(12)),
                ("invoice:three".to_string(), json!(0)),
                ("invoice:two".to_string(), json!(3)),
            ],
            "reference={reference}"
        );
        if reference {
            assert_eq!(warnings, "");
            assert!(
                select.contains("(doc<~line[WHERE vat != NONE AND vat != NULL].vat ?? [])"),
                "{select}"
            );
        } else {
            assert!(
                select.contains("FROM line WHERE doc = $parent.doc AND vat != NONE"),
                "{select}"
            );
            assert!(
                warnings.contains("roll-up over `line.doc`")
                    && warnings.contains("`DEFINE FIELD doc ON line TYPE record<doc> REFERENCE;`"),
                "{warnings}"
            );
        }
    }
}

/// A roll-up over a relation joined by more than one key is refused: one
/// correlated key would aggregate rows the other keys rule out.
#[tokio::test]
async fn rollup_over_a_multi_key_relation_is_refused() {
    let db = seeded_by_doc(true).await;
    let built = keyed_invoices(&db, "[{ from: doc, to: doc }, { from: id, to: invoice }]")
        .with_rollup("vat", "lines", RollupOp::Sum, Some("vat"), None);
    let error = built
        .expect_err("a multi-key roll-up is refused")
        .to_string();
    assert!(error.contains("more than one key"), "{error}");
}

#[tokio::test]
async fn rollups_over_a_plain_link_use_parent_and_warn() {
    let db = seeded(false).await;
    for only_live in [false, true] {
        let (table, built) = warnings_during(|| invoices(db.clone(), only_live));
        assert_eq!(built, "", "adding a roll-up renders nothing");
        let (_, warnings) = warnings_during(|| table.try_select().unwrap());
        assert_eq!(
            rollups(&table).await,
            expected(only_live),
            "only_live={only_live}"
        );
        // Once per link: the first table's first query warns, no later one.
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
