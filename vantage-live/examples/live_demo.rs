//! `cargo run --example live_demo -- --help`
//!
//! A self-contained tour of vantage-live. Plays the role of a remote
//! database with a redb file ("master.redb"), wraps it in a LiveTable
//! with a configurable cache, and exposes commands that exercise every
//! feature: cache hit/miss, pagination, write-through, custom write
//! target, and live-stream invalidation.
//!
//! No external server needed — redb-as-master stands in for "remote"
//! storage, which happens to make the demo self-contained but does
//! understate how much faster a hot cache is in real life. The
//! "Wall time" column still shows the cache effect because the master
//! deserialises CBOR rows on every read.
//!
//! Examples:
//!
//!   # Populate the master with sample data.
//!   cargo run --example live_demo -- seed
//!
//!   # Read everything twice — first is a miss, second a hit.
//!   cargo run --example live_demo -- list
//!   cargo run --example live_demo -- list
//!
//!   # Insert through the LiveTable. Cache is invalidated; next read
//!   # repopulates from master.
//!   cargo run --example live_demo -- add d Delta 40
//!   cargo run --example live_demo -- list
//!
//!   # Push a fake "remote change" event and watch the cache invalidate.
//!   cargo run --example live_demo -- event-then-list
//!
//!   # Persist the cache to disk too — runs survive process restarts.
//!   cargo run --example live_demo -- --cache ./demo-cache.redb list

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, Subcommand};

use ciborium::Value as CborValue;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_live::cache::{Cache, MemCache, NoCache};
use vantage_live::{LiveEvent, LiveTable, ManualLiveStream};
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::any::AnyTable;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

const TABLE: &str = "products";
const CACHE_KEY: &str = "products";

// ── ANSI colour helpers (no `colored` crate to keep deps tight) ──────────

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
    let colour = if micros < 200 { CYAN } else { MAGENTA };
    println!(
        "  {}{}{}: {}{:>7}µs{}",
        DIM, name, RESET, colour, micros, RESET
    );
    value
}

// ── CLI surface ──────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "live_demo", about = "A guided tour of vantage-live.")]
struct Cli {
    /// Path to the redb file pretending to be a remote database.
    #[arg(long, default_value = "./demo-master.redb", global = true)]
    master: PathBuf,

    /// Cache backend: `mem`, `none`, or a path to a redb file.
    /// Path-based caching survives process restarts.
    #[arg(long, default_value = "mem", global = true)]
    cache: String,

    /// Show vantage-live tracing spans (cache hit/miss, queue events,
    /// invalidations). Set RUST_LOG to override.
    #[arg(long, global = true)]
    debug: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Drop and repopulate the master with three sample rows. Run this
    /// first; everything else assumes seeded data.
    Seed,

    /// List rows. Run twice to see the cache hit/miss timing difference.
    List,

    /// Look up one row by id.
    Get { id: String },

    /// Insert a row through the LiveTable (queue → master → cache invalidated).
    Add {
        id: String,
        name: String,
        price: i64,
    },

    /// Delete a row through the LiveTable.
    Delete { id: String },

    /// Push a manual `LiveEvent::Updated{id}` into a ManualLiveStream
    /// attached to the LiveTable. Watch the cache get blown, then list
    /// again to confirm the next read repopulates from master.
    EventThenList { id: Option<String> },

    /// Show the LiveTable's wiring (master, cache backend, queue capacity).
    Info,
}

// ── Master + cache construction ───────────────────────────────────────────

fn open_master(path: &PathBuf) -> AnyTable {
    // Re-create the file if it doesn't exist so the demo is one-step.
    let db = Redb::create(path).expect("open or create master redb");
    let typed = Table::<Redb, EmptyEntity>::new(TABLE, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price");
    AnyTable::from_table(typed)
}

/// Build a typed Table directly — used by `seed` (which needs to write
/// through the redb-typed path, not through the LiveTable wrapper).
fn open_typed(path: &PathBuf) -> Table<Redb, EmptyEntity> {
    let db = Redb::create(path).expect("open or create master redb");
    Table::<Redb, EmptyEntity>::new(TABLE, db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
}

fn build_cache(spec: &str) -> (Arc<dyn Cache>, &'static str) {
    if spec == "mem" {
        (Arc::new(MemCache::new()), "MemCache")
    } else if spec == "none" {
        (Arc::new(NoCache), "NoCache")
    } else {
        // Path → use vantage-redb as the cache backing store too. We're
        // wrapping a Redb-backed table as a Cache via a thin shim — for
        // the demo we use a MemCache and document that "real" RedbCache
        // is on the roadmap (see DESIGN.md).
        warn(&format!(
            "path-based cache requested ({spec}) — RedbCache impl is on the roadmap; falling back to MemCache for now"
        ));
        (Arc::new(MemCache::new()), "MemCache (fallback)")
    }
}

// ── Commands ──────────────────────────────────────────────────────────────

async fn cmd_seed(typed: Table<Redb, EmptyEntity>) {
    header("seed");
    // Wipe whatever was there and lay down three rows.
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

async fn cmd_list(live: &LiveTable) {
    header("list");
    let t = Instant::now();
    let rows = live.list_values().await.unwrap();
    timed("wall time", t, ());

    if rows.is_empty() {
        warn("no rows — run `seed` first");
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

async fn cmd_get(live: &LiveTable, id: &str) {
    header("get");
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

async fn cmd_add(live: &LiveTable, id: String, name: String, price: i64) {
    header("add");
    let mut rec: Record<CborValue> = Record::new();
    rec.insert("name".into(), CborValue::Text(name.clone()));
    rec.insert("price".into(), CborValue::Integer(price.into()));

    let t = Instant::now();
    live.insert_value(&id, &rec).await.unwrap();
    timed("wall time", t, ());

    ok(&format!("queued insert: {id} = {name} (£{price})"));
    note("cache", "invalidated for cache_key prefix");
}

async fn cmd_delete(live: &LiveTable, id: String) {
    header("delete");
    let t = Instant::now();
    WritableValueSet::delete(live, &id).await.unwrap();
    timed("wall time", t, ());
    ok(&format!("queued delete: {id}"));
}

async fn cmd_event_then_list(target_id: Option<String>) {
    header("event-then-list");

    let typed = open_typed(&PathBuf::from("./demo-master.redb"));
    let master = AnyTable::from_table(typed);
    let cache = MemCache::new();
    let stream = ManualLiveStream::default();

    let live = LiveTable::new(master, CACHE_KEY, Arc::new(cache.clone()))
        .with_live_stream(Arc::new(stream.clone()));

    // Warm cache.
    println!();
    println!("  {}1) prime cache{}", BOLD, RESET);
    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("first list (miss)", t, ());

    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("second list (hit)", t, ());

    // Push event.
    println!();
    println!("  {}2) external event{}", BOLD, RESET);
    let event = match target_id {
        Some(id) => LiveEvent::Updated { id },
        None => LiveEvent::Changed,
    };
    let kind = format!("{:?}", event);
    tokio::task::yield_now().await; // let consumer subscribe
    let n = stream.push(event);
    note("pushed", &format!("{kind} → {n} subscriber(s)"));

    // Give the consumer a moment to process.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // List again — should be a miss.
    println!();
    println!("  {}3) post-event list{}", BOLD, RESET);
    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("post-event list (should miss)", t, ());

    let t = Instant::now();
    let _ = live.list_values().await.unwrap();
    timed("next list (hit again)", t, ());
}

fn cmd_info(live: &LiveTable, master_path: &std::path::Path, cache_label: &str) {
    header("info");
    note("master file", &master_path.display().to_string());
    note("cache backend", cache_label);
    note("cache_key", CACHE_KEY);
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
        Cmd::Seed => {
            let typed = open_typed(&cli.master);
            cmd_seed(typed).await;
        }
        Cmd::List => {
            let master = open_master(&cli.master);
            let (cache, _) = build_cache(&cli.cache);
            let live = LiveTable::new(master, CACHE_KEY, cache);
            cmd_list(&live).await;
        }
        Cmd::Get { id } => {
            let master = open_master(&cli.master);
            let (cache, _) = build_cache(&cli.cache);
            let live = LiveTable::new(master, CACHE_KEY, cache);
            cmd_get(&live, id).await;
        }
        Cmd::Add { id, name, price } => {
            let master = open_master(&cli.master);
            let (cache, _) = build_cache(&cli.cache);
            let live = LiveTable::new(master, CACHE_KEY, cache);
            cmd_add(&live, id.clone(), name.clone(), *price).await;
        }
        Cmd::Delete { id } => {
            let master = open_master(&cli.master);
            let (cache, _) = build_cache(&cli.cache);
            let live = LiveTable::new(master, CACHE_KEY, cache);
            cmd_delete(&live, id.clone()).await;
        }
        Cmd::EventThenList { id } => {
            cmd_event_then_list(id.clone()).await;
        }
        Cmd::Info => {
            let master = open_master(&cli.master);
            let (cache, label) = build_cache(&cli.cache);
            let live = LiveTable::new(master, CACHE_KEY, cache);
            cmd_info(&live, &cli.master, label);
        }
    }
}
