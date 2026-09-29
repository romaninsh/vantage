//! Sim engine tests. All run on a manual clock unless they say otherwise.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ciborium::Value as CborValue;
use vantage_memory::{MemoryChange, MemoryStore, MemoryTable, MemoryTableHandle};
use vantage_types::Record;

use super::*;

mod edges;
mod flow;
mod scripts;
mod spawner;
mod stats;
mod store;
mod validation;
mod warm;

/// Manual-clock origin: 2026-09-28T00:00:00Z.
const T0: u64 = 1_790_553_600;

fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(T0)
}

fn store_with(tables: &[&str]) -> MemoryStore {
    let store = MemoryStore::new();
    for t in tables {
        store.table(t);
    }
    store
}

fn rows(table: &MemoryTable) -> Vec<Record<CborValue>> {
    table
        .ids()
        .iter()
        .filter_map(|id| table.get(id))
        .map(|r| (*r).clone())
        .collect()
}

fn text(rec: &Record<CborValue>, col: &str) -> String {
    match rec.get(col) {
        Some(CborValue::Text(s)) => s.clone(),
        other => panic!("{col} is not text: {other:?}"),
    }
}

fn num(rec: &Record<CborValue>, col: &str) -> f64 {
    match rec.get(col) {
        Some(CborValue::Float(f)) => *f,
        Some(CborValue::Integer(i)) => i128::from(*i) as f64,
        other => panic!("{col} is not a number: {other:?}"),
    }
}

/// An engine on the manual clock over one `log` table.
fn engine_with(defs: Vec<SimDef>) -> (SimEngine, MemoryTableHandle) {
    let store = store_with(&["log"]);
    let mut b = SimEngine::builder()
        .store(&store)
        .manual_clock(start())
        .seed(1);
    for d in defs {
        b = b.sim(d);
    }
    (b.start().expect("engine starts"), store.table("log"))
}

/// Advance the manual clock `secs` in `step`-second steps, settling after
/// each.
fn run_for(engine: &SimEngine, secs: u64, step: u64) {
    let mut t = 0;
    while t < secs {
        engine.advance(Duration::from_secs(step));
        engine.settle();
        t += step;
    }
}
