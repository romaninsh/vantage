//! End-to-end test of the Vista CLI runner against an in-memory
//! `MockShell` Vista. Verifies every new token shape lands in the
//! correct branch:
//! - Real Vista calls (eq filter, id alias, `[N]` narrow) actually
//!   affect the result.
//! - Stubbed paths (operator beyond eq, sort, slice, search,
//!   aggregates) record a `note_stub` call so the test can assert the
//!   dispatch reached them.
//!
//! When stage 5/5b lands and the stubs are replaced with real Vista
//! calls, these tests should be updated to assert on behaviour instead
//! of stub-name strings.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::sync::Mutex;

use vantage_cli_util::output::{self, OutputFormat};
use vantage_cli_util::vista_cli::{self, AggregateOp, Mode, ModelFactory, Renderer};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{Column, Vista, VistaMetadata};

// ─── Test fixtures ─────────────────────────────────────────────────────────

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

fn seeded_shell() -> MockShell {
    // Ids are integers because MockShell does strict CBOR equality for
    // filters and the CLI's value auto-detection turns `2` into
    // `Integer(2)` (not `Text("2")`). Real backends coerce; the mock
    // doesn't, so we line up here.
    MockShell::new()
        .with_record(
            "1",
            record(&[
                ("id", CborValue::Integer(1.into())),
                ("name", cbor_text("Alice")),
                ("salary", CborValue::Integer(900.into())),
                ("vip_flag", CborValue::Bool(true)),
            ]),
        )
        .with_record(
            "2",
            record(&[
                ("id", CborValue::Integer(2.into())),
                ("name", cbor_text("Bob")),
                ("salary", CborValue::Integer(2500.into())),
                ("vip_flag", CborValue::Bool(false)),
            ]),
        )
        .with_record(
            "3",
            record(&[
                ("id", CborValue::Integer(3.into())),
                ("name", cbor_text("Carol")),
                ("salary", CborValue::Integer(1500.into())),
                ("vip_flag", CborValue::Bool(true)),
            ]),
        )
}

fn build_users_vista() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_column(Column::new("salary", "i64"))
        .with_column(Column::new("vip_flag", "bool"))
        .with_id_column("id");
    Vista::new("users", Box::new(seeded_shell()), metadata)
}

// ─── Test factory + recorder ───────────────────────────────────────────────

struct TestFactory;

impl ModelFactory for TestFactory {
    fn for_name(&self, name: &str) -> Option<(Vista, Mode)> {
        match name {
            "users" => Some((build_users_vista(), Mode::List)),
            "user" => Some((build_users_vista(), Mode::Single)),
            _ => None,
        }
    }

    fn for_locator(&self, locator: &str) -> Option<Vista> {
        // Accept `user:<id>` (SurrealDB-style Thing) and narrow the
        // standard users vista to that id. Route through the same
        // value coercion the CLI uses on `field=value` so ints stay
        // ints — see [`vista_cli::auto_detect`].
        let id = locator.strip_prefix("user:")?;
        let mut v = build_users_vista();
        v.add_condition_eq("id", vista_cli::auto_detect(id)).ok()?;
        Some(v)
    }
}

#[derive(Default)]
struct Recorder {
    stubs: Mutex<Vec<String>>,
    lists: Mutex<Vec<String>>,
    records: Mutex<Vec<String>>,
    scalars: Mutex<Vec<String>>,
    format: OutputFormat,
}

impl Recorder {
    fn with_format(format: OutputFormat) -> Self {
        Self {
            format,
            ..Default::default()
        }
    }
    fn stubs(&self) -> Vec<String> {
        self.stubs.lock().unwrap().clone()
    }
    fn lists(&self) -> Vec<String> {
        self.lists.lock().unwrap().clone()
    }
    fn records(&self) -> Vec<String> {
        self.records.lock().unwrap().clone()
    }
    fn scalars(&self) -> Vec<String> {
        self.scalars.lock().unwrap().clone()
    }
}

impl Renderer for Recorder {
    fn render_list(
        &self,
        _vista: &Vista,
        records: &IndexMap<String, Record<CborValue>>,
        _column_override: Option<&[String]>,
    ) {
        self.lists
            .lock()
            .unwrap()
            .push(output::render_list(self.format, records));
    }

    fn render_record(
        &self,
        _vista: &Vista,
        id: &str,
        record: &Record<CborValue>,
        _relations: &[String],
    ) {
        self.records
            .lock()
            .unwrap()
            .push(output::render_record(self.format, id, record));
    }

    fn render_scalar(
        &self,
        _vista: &Vista,
        op: AggregateOp,
        field: Option<&str>,
        value: &CborValue,
    ) {
        let label = match field {
            Some(f) => format!("{}({f})", op.name()),
            None => format!("{}()", op.name()),
        };
        self.scalars
            .lock()
            .unwrap()
            .push(output::render_scalar(self.format, &label, value));
    }

    fn note_stub(&self, what: &str) {
        self.stubs.lock().unwrap().push(what.to_string());
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn plain_list_renders_all_records() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users"]))
        .await
        .unwrap();

    assert!(
        rec.stubs().is_empty(),
        "no stubs expected, got {:?}",
        rec.stubs()
    );
    let lists = rec.lists();
    assert_eq!(lists.len(), 1);
    // Three records, Alice/Bob/Carol all present (cbor-diag is stable).
    assert!(lists[0].contains("\"Alice\""));
    assert!(lists[0].contains("\"Bob\""));
    assert!(lists[0].contains("\"Carol\""));
}

#[tokio::test]
async fn eq_filter_narrows_results() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "vip_flag=true"]))
        .await
        .unwrap();

    let lists = rec.lists();
    assert_eq!(lists.len(), 1);
    assert!(lists[0].contains("\"Alice\""));
    assert!(!lists[0].contains("\"Bob\""));
    assert!(lists[0].contains("\"Carol\""));
}

#[tokio::test]
async fn typed_bool_works_same_as_autodetect() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "vip_flag=#true"]))
        .await
        .unwrap();

    let lists = rec.lists();
    assert!(lists[0].contains("\"Alice\""));
    assert!(!lists[0].contains("\"Bob\""));
}

#[tokio::test]
async fn id_alias_forces_single_mode() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "id=2"]))
        .await
        .unwrap();

    assert!(rec.lists().is_empty());
    let records = rec.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].contains("\"Bob\""));
}

#[tokio::test]
async fn index_narrows_to_single() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users[0]"]))
        .await
        .unwrap();

    let records = rec.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].contains("\"Alice\""));
}

#[tokio::test]
async fn operator_lt_reaches_stub() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "salary:lt=1000"]))
        .await
        .unwrap();

    let stubs = rec.stubs();
    assert_eq!(stubs.len(), 1);
    assert!(stubs[0].starts_with("add_condition(\"salary\", lt"));
    // Filter not applied yet — all three records still come back.
    let lists = rec.lists();
    assert_eq!(lists.len(), 1);
    assert!(lists[0].contains("\"Bob\""));
}

#[tokio::test]
async fn nullary_op_reaches_stub() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "manager_id:null"]))
        .await
        .unwrap();

    let stubs = rec.stubs();
    assert_eq!(
        stubs,
        vec!["add_condition(\"manager_id\", null)".to_string()]
    );
}

#[tokio::test]
async fn sort_reaches_stub() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "[+name]"]))
        .await
        .unwrap();

    let stubs = rec.stubs();
    assert_eq!(stubs.len(), 1);
    assert!(stubs[0].starts_with("add_order(\"name\", Asc)"));
}

#[tokio::test]
async fn slice_range_reaches_stub() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "[0:2]"]))
        .await
        .unwrap();

    let stubs = rec.stubs();
    assert_eq!(stubs, vec!["set_pagination(0, Some(2))".to_string()]);
}

#[tokio::test]
async fn sort_then_index_narrows_to_single() {
    // [+name:0] — sort stub fires, then `apply_index` narrows to a row.
    // With sort unwired the chosen row is just the first in seed order
    // (id "1" = Alice), but the *narrowing* is the wired part being
    // verified here.
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users[+name:0]"]))
        .await
        .unwrap();

    let stubs = rec.stubs();
    assert_eq!(stubs, vec!["add_order(\"name\", Asc)".to_string()]);
    let records = rec.records();
    assert_eq!(records.len(), 1);
    // The first record in seed order is Alice. Once sort is wired,
    // this will become "Alice" by name-ascending sort — same record
    // here, but the assertion will need updating for fixtures whose
    // seed order differs from alphabetical.
    assert!(records[0].contains("\"Alice\""));
}

#[tokio::test]
async fn search_reaches_stub() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "?alice"]))
        .await
        .unwrap();

    let stubs = rec.stubs();
    assert_eq!(stubs, vec!["add_search(\"alice\")".to_string()]);
}

#[tokio::test]
async fn aggregate_short_circuits_to_scalar() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "@sum:salary"]))
        .await
        .unwrap();

    let stubs = rec.stubs();
    assert_eq!(stubs, vec!["sum(salary)".to_string()]);
    assert!(rec.lists().is_empty());
    assert!(rec.records().is_empty());
    let scalars = rec.scalars();
    assert_eq!(scalars.len(), 1);
    // Stubbed value is null until vista.get_sum lands.
    assert!(scalars[0].contains("null"));
}

#[tokio::test]
async fn aggregate_must_be_terminal() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    let err = vista_cli::run(
        &TestFactory,
        &rec,
        &argv(&["users", "@sum:salary", "vip_flag=true"]),
    )
    .await
    .unwrap_err();
    assert!(format!("{err}").contains("Aggregate token"));
}

#[tokio::test]
async fn locator_resolves_via_for_locator() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["user:2"]))
        .await
        .unwrap();

    let records = rec.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].contains("\"Bob\""));
}

#[tokio::test]
async fn locator_unknown_scheme_errors() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    let err = vista_cli::run(&TestFactory, &rec, &argv(&["arn:aws:unknown"]))
        .await
        .unwrap_err();
    assert!(format!("{err}").contains("Cannot resolve locator"));
}

#[tokio::test]
async fn cbor_diag_output_round_trips_record() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "id=1"]))
        .await
        .unwrap();

    let records = rec.records();
    // Strict equality on cbor-diag — this is the format third-party
    // drivers must match byte-for-byte in their bakery example tests.
    // `Record<CborValue>` preserves insertion order from the seed, so
    // the field ordering is stable.
    assert_eq!(
        records[0],
        "\"1\": {\"id\": 1, \"name\": \"Alice\", \"salary\": 900, \"vip_flag\": true}\n"
    );
}

#[tokio::test]
async fn json_output_loses_int_but_keeps_bool() {
    let rec = Recorder::with_format(OutputFormat::Json);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "id=1"]))
        .await
        .unwrap();

    let records = rec.records();
    // JSON output: bool stays, int stays (fits in i64), strings quoted.
    assert_eq!(
        records[0],
        "{\"1\":{\"id\":1,\"name\":\"Alice\",\"salary\":900,\"vip_flag\":true}}\n"
    );
}

#[tokio::test]
async fn ndjson_output_one_line_per_record() {
    let rec = Recorder::with_format(OutputFormat::Ndjson);
    vista_cli::run(&TestFactory, &rec, &argv(&["users"]))
        .await
        .unwrap();

    let lists = rec.lists();
    assert_eq!(lists.len(), 1);
    let lines: Vec<&str> = lists[0].lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("{\"_id\":\"1\","));
}
