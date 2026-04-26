//! `cargo run --example live_demo -- --help`
//!
//! Self-contained tour of vantage-live with two master modes:
//!
//! - **`local`** — a redb file pretending to be a remote database.
//!   Self-contained, supports the full read/write/event cycle.
//! - **`api <users|posts|comments>`** — JSONPlaceholder
//!   (https://jsonplaceholder.typicode.com), a free public REST API.
//!   Read-only; the cache benefit is far more dramatic here because
//!   network round-trips dwarf local CBOR decoding.
//!
//! Examples:
//!
//!   # Local redb master, full cycle.
//!   cargo run --example live_demo -- local seed
//!   cargo run --example live_demo -- local list           # cache miss
//!   cargo run --example live_demo -- local list           # cache hit
//!   cargo run --example live_demo -- local add d Donut 5
//!   cargo run --example live_demo -- local event-then-list
//!
//!   # JSONPlaceholder master. First call hits the network (~50–300ms);
//!   # subsequent calls are microseconds-fast.
//!   cargo run --example live_demo -- api users list
//!   cargo run --example live_demo -- api users get 1
//!   cargo run --example live_demo -- api posts list
//!
//!   # Disk-persistent cache: state survives process restarts.
//!   cargo run --example live_demo -- --cache ./vlive-cache api users list
//!   cargo run --example live_demo -- --cache ./vlive-cache api users list
//!
//!   # Pagination: each page caches separately.
//!   cargo run --example live_demo -- api users list --page 1 --limit 3
//!   cargo run --example live_demo -- api users list --page 2 --limit 3
//!
//!   # Tracing.
//!   cargo run --example live_demo -- --debug api users list

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, Subcommand};

use ciborium::Value as CborValue;
use vantage_api_client::{ResponseShape, RestApi};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_live::cache::{Cache, MemCache, NoCache, RedbCache};
use vantage_live::{LiveEvent, LiveTable, ManualLiveStream};
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::any::AnyTable;
use vantage_table::pagination::Pagination;
use vantage_table::table::Table;
use vantage_table::traits::table_like::TableLike;
use vantage_types::{EmptyEntity, Record};

const LOCAL_TABLE: &str = "products";
const LOCAL_CACHE_KEY: &str = "products";
const JSONPLACEHOLDER: &str = "https://jsonplaceholder.typicode.com";

// ── ANSI colour helpers ──────────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";

fn header(text: &str) {
    println!("{}{}━━ {} ━━{}", BOLD, BLUE, text, RESET);
}

fn note(label: &str, text: &str) {
    println!("  {}{}{}: {}", DIM, label, RESET, text);
}

fn ok(text: &str) {
    println!("  {}✓{} {}", GREEN, RESET, text);
}

fn warn(text: &str) {
    println!("  {}!{} {}", YELLOW, RESET, text);
}

fn timed<T>(name: &str, t: Instant, value: T) -> T {
    let elapsed = t.elapsed();
    let micros = elapsed.as_micros();
    let secs = micros / 1_000_000;
    let millis = (micros / 1_000) % 1_000;
    // Sub-ms cache hits show as 00.000 — still tells the story when the
    // miss right above it is e.g. 00.446. Colour bands kick in once
    // we're above 1ms.
    let colour = if micros < 1_000 {
        CYAN
    } else if micros < 10_000 {
        YELLOW
    } else {
        MAGENTA
    };
    println!(
        "  {}{}{}: {}{:>2}.{:03}{} s",
        DIM, name, RESET, colour, secs, millis, RESET
    );
    value
}

// ── CLI surface ──────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "live_demo", about = "A guided tour of vantage-live.")]
struct Cli {
    /// Path to the redb file pretending to be a remote database
    /// (used by the `local` subcommand).
    #[arg(long, default_value = "./demo-master.redb", global = true)]
    master: PathBuf,

    /// Cache backend: `mem`, `none`, or a path to a folder. The folder
    /// becomes a RedbCache (one redb file inside, persisting across
    /// process restarts).
    #[arg(long, default_value = "mem", global = true)]
    cache: String,

    /// Show vantage-live tracing spans (cache hit/miss, queue events,
    /// invalidations). Set `RUST_LOG` to override the filter.
    #[arg(long, global = true)]
    debug: bool,

    #[command(subcommand)]
    cmd: TopCmd,
}

#[derive(Subcommand, Debug)]
enum TopCmd {
    /// Local redb master — full read / write / live-event cycle.
    Local {
        #[command(subcommand)]
        cmd: LocalCmd,
    },
    /// JSONPlaceholder API master — read-only, but the cache effect is
    /// dramatic (network → microseconds).
    Api {
        /// Resource: `users`, `posts`, or `comments`. (Any
        /// JSONPlaceholder collection works, but these three are the
        /// ones the demo formats nicely.)
        #[arg(value_parser = ["users", "posts", "comments", "albums", "todos"])]
        resource: String,

        #[command(subcommand)]
        cmd: ApiCmd,
    },
}

#[derive(Subcommand, Debug)]
enum LocalCmd {
    /// Drop and repopulate the master with three sample rows.
    Seed,
    /// List rows. Run twice to see the cache hit/miss timing.
    List,
    /// Look up one row by id.
    Get { id: String },
    /// Insert a row through the LiveTable.
    Add {
        id: String,
        name: String,
        price: i64,
    },
    /// Delete a row through the LiveTable.
    Delete { id: String },
    /// Push a manual `LiveEvent` and watch the cache invalidate.
    EventThenList { id: Option<String> },
    /// Show the LiveTable's wiring.
    Info,
}

#[derive(Subcommand, Debug)]
enum ApiCmd {
    /// List rows from the resource. Run twice for cache hit/miss
    /// timing — the difference is on the order of 1000x.
    List {
        /// 1-based page number. JSONPlaceholder uses `?_page=N&_limit=M`.
        #[arg(long)]
        page: Option<i64>,
        /// Items per page.
        #[arg(long)]
        limit: Option<i64>,
        /// Filter rows by an `eq` condition, e.g. `--filter postId=1`
        /// or `--filter completed=true`. The condition is pushed into
        /// the URL query string by vantage-api-client and folded into
        /// the cache key — different filters cache under different
        /// keys, which is what the caller-owned-cache-key contract
        /// requires.
        #[arg(long, value_parser = parse_filter)]
        filter: Option<(String, String)>,
    },
    /// Fetch one record by id (e.g. `1`).
    Get { id: String },
    /// Show the LiveTable's wiring for this API resource.
    Info,
}

/// Parse a `field=value` flag value into a tuple. Used by `--filter`.
fn parse_filter(s: &str) -> std::result::Result<(String, String), String> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| format!("expected `field=value`, got `{s}`"))?;
    if k.is_empty() {
        return Err("empty field name".into());
    }
    Ok((k.to_string(), v.to_string()))
}

// ── Local master + cache ──────────────────────────────────────────────────

fn open_local_master(path: &Path) -> AnyTable {
    let db = Redb::create(path).expect("open or create master redb");
    let typed = Table::<Redb, EmptyEntity>::new(LOCAL_TABLE, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    AnyTable::from_table(typed)
}

fn open_local_typed(path: &Path) -> Table<Redb, EmptyEntity> {
    let db = Redb::create(path).expect("open or create master redb");
    Table::<Redb, EmptyEntity>::new(LOCAL_TABLE, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
}

// ── API master ────────────────────────────────────────────────────────────
//
// `RestApi::Value = serde_json::Value`, but `AnyTable` carries
// `ciborium::Value`. There's no `Into<CborValue> + From<CborValue>` impl
// for `serde_json::Value` (both are foreign types), so `from_table` rejects
// `Table<RestApi, _>` directly. Wrap in a tiny adapter that implements
// `TableLike<Value = CborValue, Id = String>` and converts on the fly via
// serde round-trip. This is demo code — vantage-api-client could grow a
// proper bridge as a follow-up.

fn open_api_master(resource: &str, filter: Option<&(String, String)>) -> AnyTable {
    use vantage_api_client::eq_condition;

    let api = RestApi::builder(JSONPLACEHOLDER)
        .response_shape(ResponseShape::BareArray)
        .build();
    let mut typed = Table::<RestApi, EmptyEntity>::new(resource, api).with_id_column("id");

    if let Some((field, value)) = filter {
        // Try the value as a number first, then bool, otherwise pass as
        // a string. JSON Server is type-permissive in URL params, so
        // this mostly works regardless, but `?completed=true` for a
        // boolean field is more conventional than `?completed="true"`.
        let json_value = if let Ok(n) = value.parse::<i64>() {
            serde_json::Value::Number(n.into())
        } else if value == "true" {
            serde_json::Value::Bool(true)
        } else if value == "false" {
            serde_json::Value::Bool(false)
        } else {
            serde_json::Value::String(value.clone())
        };
        typed.add_condition(eq_condition(field, json_value));
    }

    AnyTable::from_table_like(api_master::JsonToCborAdapter::new(typed))
}

/// Build a cache key that varies with the filter. With no filter,
/// `users`. With `--filter postId=1`, `comments?postId=1`. Caller
/// ownership of cache_key is the design rule (DESIGN.md) — different
/// conditions need different keys, otherwise the cache would serve
/// stale-shape data when filters change.
fn api_cache_key(resource: &str, filter: Option<&(String, String)>) -> String {
    match filter {
        Some((f, v)) => format!("{resource}?{f}={v}"),
        None => resource.to_string(),
    }
}

mod api_master {
    use async_trait::async_trait;
    use ciborium::Value as CborValue;
    use indexmap::IndexMap;
    use vantage_api_client::RestApi;
    use vantage_core::{Result, error};
    use vantage_dataset::traits::{ReadableValueSet, ValueSet, WritableValueSet};
    use vantage_expressions::AnyExpression;
    use vantage_table::conditions::ConditionHandle;
    use vantage_table::pagination::Pagination;
    use vantage_table::table::Table;
    use vantage_table::traits::table_like::TableLike;
    use vantage_types::{EmptyEntity, Record};

    /// Demo-only TableLike adapter. Wraps `Table<RestApi, EmptyEntity>`
    /// and converts each row's `serde_json::Value` fields to/from
    /// `ciborium::Value` so the master fits AnyTable's CBOR-shaped slot.
    #[derive(Clone)]
    pub struct JsonToCborAdapter {
        inner: Table<RestApi, EmptyEntity>,
    }

    impl JsonToCborAdapter {
        pub fn new(inner: Table<RestApi, EmptyEntity>) -> Self {
            Self { inner }
        }
    }

    fn json_to_cbor(v: serde_json::Value) -> CborValue {
        // serde round-trip — same lossy bits as elsewhere (NaN, binary
        // → string), but JSONPlaceholder responses are vanilla JSON so
        // this is fine.
        ciborium::Value::serialized(&v).unwrap_or(CborValue::Null)
    }

    fn cbor_to_json(v: CborValue) -> serde_json::Value {
        serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
    }

    fn record_j2c(r: Record<serde_json::Value>) -> Record<CborValue> {
        r.into_iter().map(|(k, v)| (k, json_to_cbor(v))).collect()
    }

    fn record_c2j(r: Record<CborValue>) -> Record<serde_json::Value> {
        r.into_iter().map(|(k, v)| (k, cbor_to_json(v))).collect()
    }

    impl ValueSet for JsonToCborAdapter {
        type Id = String;
        type Value = CborValue;
    }

    #[async_trait]
    impl ReadableValueSet for JsonToCborAdapter {
        async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>> {
            let rows = self.inner.list_values().await?;
            Ok(rows.into_iter().map(|(k, v)| (k, record_j2c(v))).collect())
        }

        async fn get_value(&self, id: &String) -> Result<Option<Record<CborValue>>> {
            Ok(self.inner.get_value(id).await?.map(record_j2c))
        }

        async fn get_some_value(&self) -> Result<Option<(String, Record<CborValue>)>> {
            Ok(self
                .inner
                .get_some_value()
                .await?
                .map(|(k, v)| (k, record_j2c(v))))
        }
    }

    #[async_trait]
    impl WritableValueSet for JsonToCborAdapter {
        async fn insert_value(
            &self,
            id: &String,
            record: &Record<CborValue>,
        ) -> Result<Record<CborValue>> {
            // Round-trip through inner write path so existing semantics
            // (read-only error) propagate naturally.
            let json_record = record_c2j(record.clone());
            let result = self.inner.insert_value(id, &json_record).await?;
            Ok(record_j2c(result))
        }

        async fn replace_value(
            &self,
            id: &String,
            record: &Record<CborValue>,
        ) -> Result<Record<CborValue>> {
            let json_record = record_c2j(record.clone());
            let result = self.inner.replace_value(id, &json_record).await?;
            Ok(record_j2c(result))
        }

        async fn patch_value(
            &self,
            id: &String,
            partial: &Record<CborValue>,
        ) -> Result<Record<CborValue>> {
            let json_partial = record_c2j(partial.clone());
            let result = self.inner.patch_value(id, &json_partial).await?;
            Ok(record_j2c(result))
        }

        async fn delete(&self, id: &String) -> Result<()> {
            self.inner.delete(id).await
        }

        async fn delete_all(&self) -> Result<()> {
            self.inner.delete_all().await
        }
    }

    #[async_trait]
    impl TableLike for JsonToCborAdapter {
        fn table_name(&self) -> &str {
            self.inner.table_name()
        }
        fn table_alias(&self) -> &str {
            self.inner.table_alias()
        }
        fn column_names(&self) -> Vec<String> {
            self.inner.column_names()
        }
        fn add_condition(
            &mut self,
            _condition: Box<dyn std::any::Any + Send + Sync>,
        ) -> Result<()> {
            Err(error!(
                "JsonToCborAdapter (demo): condition pushdown not implemented"
            ))
        }
        fn temp_add_condition(&mut self, _c: AnyExpression) -> Result<ConditionHandle> {
            Err(error!(
                "JsonToCborAdapter (demo): temp_add_condition not implemented"
            ))
        }
        fn temp_remove_condition(&mut self, _h: ConditionHandle) -> Result<()> {
            Err(error!(
                "JsonToCborAdapter (demo): temp_remove_condition not implemented"
            ))
        }
        fn search_expression(&self, _: &str) -> Result<AnyExpression> {
            Err(error!(
                "JsonToCborAdapter (demo): search_expression not implemented"
            ))
        }
        fn clone_box(&self) -> Box<dyn TableLike<Value = CborValue, Id = String>> {
            Box::new(self.clone())
        }
        fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
            self
        }
        fn as_any_ref(&self) -> &dyn std::any::Any {
            self
        }
        fn set_pagination(&mut self, p: Option<Pagination>) {
            self.inner.set_pagination(p);
        }
        fn get_pagination(&self) -> Option<&Pagination> {
            self.inner.get_pagination()
        }
        async fn get_count(&self) -> Result<i64> {
            self.inner.get_count().await
        }
    }
}

// ── Cache builder ────────────────────────────────────────────────────────

fn build_cache(spec: &str) -> (Arc<dyn Cache>, String) {
    if spec == "mem" {
        (Arc::new(MemCache::new()), "MemCache".into())
    } else if spec == "none" {
        (Arc::new(NoCache), "NoCache".into())
    } else {
        let cache = RedbCache::open(spec).expect("open or create RedbCache folder");
        (Arc::new(cache), format!("RedbCache({spec})"))
    }
}

// ── Local commands ───────────────────────────────────────────────────────

async fn cmd_local_seed(typed: Table<Redb, EmptyEntity>) {
    header("local: seed");
    let _ = WritableValueSet::delete_all(&typed).await;

    for (id, name, price) in [
        ("a", "Apple Pie", 12),
        ("b", "Brioche", 8),
        ("c", "Cinnamon Roll", 15),
    ] {
        let mut r: Record<AnyRedbType> = Record::new();
        r.insert("name".into(), AnyRedbType::new(name.to_string()));
        r.insert("price".into(), AnyRedbType::new(price as i64));
        typed.insert_value(&id.to_string(), &r).await.unwrap();
        ok(&format!("inserted {id} = {name} (£{price})"));
    }
}

async fn cmd_local_list(live: &LiveTable) {
    header("local: list");
    let t = Instant::now();
    let rows = live.list_values().await.unwrap();
    timed("wall time", t, ());

    if rows.is_empty() {
        warn("no rows — run `local seed` first");
        return;
    }

    println!();
    println!(
        "  {}{:<8} {:<20} {:>8}{}",
        BOLD, "id", "name", "price", RESET
    );
    println!("  {}", "─".repeat(40));
    for (id, row) in &rows {
        let name = row.get("name").and_then(cbor_text).unwrap_or("—".into());
        let price = row.get("price").and_then(cbor_int).unwrap_or(0);
        println!("  {:<8} {:<20} £{:>6}", id, name, price);
    }
    println!();
    note("rows", &rows.len().to_string());
}

async fn cmd_local_get(live: &LiveTable, id: &str) {
    header("local: get");
    let t = Instant::now();
    let row = live.get_value(&id.to_string()).await.unwrap();
    timed("wall time", t, ());

    match row {
        Some(r) => {
            for (k, v) in r.iter() {
                println!("  {}{}{} = {}", DIM, k, RESET, fmt_cbor(v));
            }
        }
        None => warn(&format!("no row with id `{id}`")),
    }
}

async fn cmd_local_add(live: &LiveTable, id: String, name: String, price: i64) {
    header("local: add");
    let mut rec: Record<CborValue> = Record::new();
    rec.insert("name".into(), CborValue::Text(name.clone()));
    rec.insert("price".into(), CborValue::Integer(price.into()));

    let t = Instant::now();
    live.insert_value(&id, &rec).await.unwrap();
    timed("wall time", t, ());

    ok(&format!("queued insert: {id} = {name} (£{price})"));
    note("cache", "invalidated for cache_key prefix");
}

async fn cmd_local_delete(live: &LiveTable, id: String) {
    header("local: delete");
    let t = Instant::now();
    WritableValueSet::delete(live, &id).await.unwrap();
    timed("wall time", t, ());
    ok(&format!("queued delete: {id}"));
}

async fn cmd_local_event_then_list(target_id: Option<String>, master_path: &Path) {
    header("local: event-then-list");

    let master = open_local_master(master_path);
    let cache = MemCache::new();
    let stream = ManualLiveStream::default();

    let live = LiveTable::new(master, LOCAL_CACHE_KEY, Arc::new(cache.clone()))
        .with_live_stream(Arc::new(stream.clone()));

    println!();
    println!("  {}1) prime cache{}", BOLD, RESET);
    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("first list (miss)", t, ());

    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("second list (hit)", t, ());

    println!();
    println!("  {}2) external event{}", BOLD, RESET);
    let event = match target_id {
        Some(id) => LiveEvent::Updated { id },
        None => LiveEvent::Changed,
    };
    let kind = format!("{:?}", event);
    tokio::task::yield_now().await;
    let n = stream.push(event);
    note("pushed", &format!("{kind} → {n} subscriber(s)"));

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    println!();
    println!("  {}3) post-event list{}", BOLD, RESET);
    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("post-event list (should miss)", t, ());

    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("next list (hit again)", t, ());
}

// ── API commands ─────────────────────────────────────────────────────────

async fn cmd_api_list(
    resource: &str,
    page: Option<i64>,
    limit: Option<i64>,
    filter: Option<&(String, String)>,
    cache_spec: &str,
) {
    header(&format!("api: {resource} list"));
    let master = open_api_master(resource, filter);
    let cache_key = api_cache_key(resource, filter);
    let (cache, _) = build_cache(cache_spec);
    let mut live = LiveTable::new(master, &cache_key, cache);

    if let (Some(p), Some(l)) = (page, limit) {
        live.set_pagination(Some(Pagination::new(p, l)));
        note("pagination", &format!("page {p}, {l} per page"));
    }
    if let Some((f, v)) = filter {
        note("filter", &format!("{f} = {v}"));
    }

    let t = Instant::now();
    let rows = live.list_values().await.unwrap();
    timed("wall time", t, ());

    if rows.is_empty() {
        warn("no rows returned");
        return;
    }

    let columns = pretty_columns_for(resource);
    print_api_table(&columns, &rows);
    println!();
    note("rows", &rows.len().to_string());
    note(
        "cache key",
        &live.page_cache_key(live.get_pagination().map(|p| p.get_page()).unwrap_or(1)),
    );
}

async fn cmd_api_get(resource: &str, id: &str, cache_spec: &str) {
    header(&format!("api: {resource} get {id}"));
    let master = open_api_master(resource, None);
    let (cache, _) = build_cache(cache_spec);
    let live = LiveTable::new(master, resource, cache);

    let t = Instant::now();
    let row = live.get_value(&id.to_string()).await.unwrap();
    timed("wall time", t, ());

    match row {
        Some(r) => {
            for (k, v) in r.iter() {
                println!("  {}{}{} = {}", DIM, k, RESET, fmt_json(v));
            }
        }
        None => warn(&format!("no row with id `{id}`")),
    }
}

fn cmd_api_info(resource: &str, cache_label: &str) {
    header(&format!("api: {resource} info"));
    note("base url", JSONPLACEHOLDER);
    note("resource", resource);
    note("response shape", "BareArray");
    note("cache backend", cache_label);
    println!();
    println!("  {}┌─ LiveTable wiring{}", DIM, RESET);
    println!(
        "  {}│{}  master  → AnyTable<RestApi, EmptyEntity> → {}/{}",
        DIM, RESET, JSONPLACEHOLDER, resource
    );
    println!("  {}│{}  cache   → {}", DIM, RESET, cache_label);
    println!("  {}│{}  writes  → master is read-only", DIM, RESET);
    println!("  {}└─ all reads consult cache first{}", DIM, RESET);
}

// ── Pretty-print columns per resource ─────────────────────────────────────

struct Columns {
    headers: &'static [(&'static str, usize)],
}

fn pretty_columns_for(resource: &str) -> Columns {
    match resource {
        "users" => Columns {
            headers: &[("id", 4), ("name", 24), ("username", 16), ("email", 28)],
        },
        "posts" => Columns {
            headers: &[("id", 4), ("userId", 8), ("title", 60)],
        },
        "comments" => Columns {
            headers: &[("id", 4), ("postId", 8), ("name", 30), ("email", 28)],
        },
        _ => Columns {
            headers: &[("id", 4), ("title", 50)],
        },
    }
}

fn print_api_table(cols: &Columns, rows: &indexmap::IndexMap<String, Record<CborValue>>) {
    println!();
    print!("  ");
    for (h, w) in cols.headers {
        print!("{}{:<width$}{} ", BOLD, h, RESET, width = w);
    }
    println!();
    let total_width: usize = cols.headers.iter().map(|(_, w)| w + 1).sum();
    println!("  {}", "─".repeat(total_width));

    for row in rows.values() {
        print!("  ");
        for (h, w) in cols.headers {
            let val = row.get(*h).map(fmt_json).unwrap_or_else(|| "—".into());
            let truncated = truncate(&val, *w);
            print!("{:<width$} ", truncated, width = w);
        }
        println!();
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

// ── Local info ────────────────────────────────────────────────────────────

fn cmd_local_info(live: &LiveTable, master_path: &Path, cache_label: &str) {
    header("local: info");
    note("master file", &master_path.display().to_string());
    note("cache backend", cache_label);
    note("cache_key", LOCAL_CACHE_KEY);
    note("debug formatted", &format!("{:?}", live));
    println!();
    println!("  {}┌─ LiveTable wiring{}", DIM, RESET);
    println!("  {}│{}  master  → AnyTable<Redb, EmptyEntity>", DIM, RESET);
    println!("  {}│{}  cache   → {}", DIM, RESET, cache_label);
    println!("  {}│{}  writes  → mpsc queue → master", DIM, RESET);
    println!("  {}└─ all reads consult cache first{}", DIM, RESET);
}

// ── helpers ──────────────────────────────────────────────────────────────

fn cbor_text(v: &CborValue) -> Option<String> {
    if let CborValue::Text(s) = v {
        Some(s.clone())
    } else {
        None
    }
}
fn cbor_int(v: &CborValue) -> Option<i64> {
    if let CborValue::Integer(i) = v {
        i64::try_from(*i).ok()
    } else {
        None
    }
}
fn fmt_cbor(v: &CborValue) -> String {
    match v {
        CborValue::Null => "null".into(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Integer(i) => format!("{:?}", i),
        CborValue::Float(f) => format!("{f}"),
        CborValue::Text(s) => format!("\"{s}\""),
        CborValue::Bytes(b) => format!("Bytes({} bytes)", b.len()),
        other => format!("{:?}", other),
    }
}

/// Pretty-print a CBOR value the way a JSON-shaped API would. Numbers
/// and booleans unquoted, strings with no quotes (table cell already
/// has the column for context), other shapes via Debug.
fn fmt_json(v: &CborValue) -> String {
    match v {
        CborValue::Null => "null".into(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Integer(i) => match i64::try_from(*i) {
            Ok(n) => n.to_string(),
            Err(_) => format!("{:?}", i),
        },
        CborValue::Float(f) => format!("{f}"),
        CborValue::Text(s) => s.clone(),
        other => format!("{:?}", other),
    }
}

// ── main ──────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if cli.debug {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("vantage_live=debug")),
            )
            .with_target(true)
            .compact()
            .try_init();
    }

    match &cli.cmd {
        TopCmd::Local { cmd } => match cmd {
            LocalCmd::Seed => {
                let typed = open_local_typed(&cli.master);
                cmd_local_seed(typed).await;
            }
            LocalCmd::List => {
                let master = open_local_master(&cli.master);
                let (cache, _) = build_cache(&cli.cache);
                let live = LiveTable::new(master, LOCAL_CACHE_KEY, cache);
                cmd_local_list(&live).await;
            }
            LocalCmd::Get { id } => {
                let master = open_local_master(&cli.master);
                let (cache, _) = build_cache(&cli.cache);
                let live = LiveTable::new(master, LOCAL_CACHE_KEY, cache);
                cmd_local_get(&live, id).await;
            }
            LocalCmd::Add { id, name, price } => {
                let master = open_local_master(&cli.master);
                let (cache, _) = build_cache(&cli.cache);
                let live = LiveTable::new(master, LOCAL_CACHE_KEY, cache);
                cmd_local_add(&live, id.clone(), name.clone(), *price).await;
            }
            LocalCmd::Delete { id } => {
                let master = open_local_master(&cli.master);
                let (cache, _) = build_cache(&cli.cache);
                let live = LiveTable::new(master, LOCAL_CACHE_KEY, cache);
                cmd_local_delete(&live, id.clone()).await;
            }
            LocalCmd::EventThenList { id } => {
                cmd_local_event_then_list(id.clone(), &cli.master).await;
            }
            LocalCmd::Info => {
                let master = open_local_master(&cli.master);
                let (cache, label) = build_cache(&cli.cache);
                let live = LiveTable::new(master, LOCAL_CACHE_KEY, cache);
                cmd_local_info(&live, &cli.master, &label);
            }
        },
        TopCmd::Api { resource, cmd } => match cmd {
            ApiCmd::List {
                page,
                limit,
                filter,
            } => {
                cmd_api_list(resource, *page, *limit, filter.as_ref(), &cli.cache).await;
            }
            ApiCmd::Get { id } => {
                cmd_api_get(resource, id, &cli.cache).await;
            }
            ApiCmd::Info => {
                let (_, label) = build_cache(&cli.cache);
                cmd_api_info(resource, &label);
            }
        },
    }
}
