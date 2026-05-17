//! End-to-end test of the Vista CLI runner against an in-memory
//! `MockShell` Vista. Covers the paths wired to real `Vista` calls:
//! eq filter, `id=` alias, `[N]` narrow, `[+col]` sort, `?keyword`
//! search, locator dispatch, and the three output formats. Parser
//! behaviour for the still-stubbed paths (non-`eq` operators,
//! `[N:M]` range slicing, aggregates) is covered in `parse.rs` unit
//! tests — no need to re-assert it here.

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
        .with_column(
            Column::new("name", "String")
                .with_flag("title")
                .with_flag("orderable")
                .with_flag("searchable"),
        )
        .with_column(Column::new("salary", "i64").with_flag("orderable"))
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
async fn sort_applies_via_add_order() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "[+name]"]))
        .await
        .unwrap();

    assert!(
        rec.stubs().is_empty(),
        "sort is wired to Vista::add_order, no stub expected: {:?}",
        rec.stubs()
    );
    // MockShell honours add_order, so the rendered list comes back in
    // name-ascending order: Alice, Bob, Carol.
    let lists = rec.lists();
    assert_eq!(lists.len(), 1);
    let rendered = &lists[0];
    let alice = rendered.find("Alice").expect("Alice present");
    let bob = rendered.find("Bob").expect("Bob present");
    let carol = rendered.find("Carol").expect("Carol present");
    assert!(alice < bob && bob < carol, "got: {rendered}");
}

#[tokio::test]
async fn sort_then_index_narrows_to_single() {
    // [+name:0] — sort, then `apply_index` narrows to the top row. With
    // sort wired through `Vista::add_order`, "row 0" is the smallest
    // by name-ascending, which is Alice (matches seed order here too).
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users[+name:0]"]))
        .await
        .unwrap();

    assert!(rec.stubs().is_empty(), "stubs: {:?}", rec.stubs());
    let records = rec.records();
    assert_eq!(records.len(), 1);
    assert!(records[0].contains("\"Alice\""));
}

#[tokio::test]
async fn search_applies_via_add_search() {
    let rec = Recorder::with_format(OutputFormat::CborDiag);
    vista_cli::run(&TestFactory, &rec, &argv(&["users", "?alice"]))
        .await
        .unwrap();

    assert!(
        rec.stubs().is_empty(),
        "search is wired to Vista::add_search, no stub expected: {:?}",
        rec.stubs()
    );
    // MockShell honours add_search with case-insensitive substring
    // matching across text fields — so "?alice" leaves only Alice.
    let lists = rec.lists();
    assert_eq!(lists.len(), 1);
    let rendered = &lists[0];
    assert!(rendered.contains("Alice"), "got: {rendered}");
    assert!(!rendered.contains("Bob"), "got: {rendered}");
    assert!(!rendered.contains("Carol"), "got: {rendered}");
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
