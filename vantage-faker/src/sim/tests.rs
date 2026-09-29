//! Sim engine tests. All run on a manual clock unless they say otherwise.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ciborium::Value as CborValue;
use tokio::sync::broadcast;
use vantage_diorama::ChangeEvent;
use vantage_types::Record;
use vantage_vista::mocks::MockShell;

use super::*;
use crate::{FakerColumn, FakerCtx};

mod edges;
mod flow;
mod scripts;
mod spawner;
mod stats;
mod validation;
mod warm;

/// Manual-clock origin: 2026-09-28T00:00:00Z.
const T0: u64 = 1_790_553_600;

fn start() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(T0)
}

/// A table store with `columns` (the first is the id).
fn table(columns: &[&str]) -> (Arc<FakerCtx>, broadcast::Receiver<ChangeEvent>) {
    let (tx, rx) = broadcast::channel(1 << 16);
    let cols = columns
        .iter()
        .map(|c| FakerColumn::new(*c, "string"))
        .collect();
    let id = columns[0].to_string();
    (Arc::new(FakerCtx::new(MockShell::new(), tx, cols, id)), rx)
}

fn rows(ctx: &FakerCtx) -> Vec<Record<CborValue>> {
    ctx.record_ids()
        .iter()
        .filter_map(|id| ctx.get_record(id))
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
fn engine_with(defs: Vec<SimDef>) -> (SimEngine, Arc<FakerCtx>) {
    let (log, _) = table(&["id", "who", "step", "at"]);
    let mut b = SimEngine::builder()
        .table("log", &log)
        .manual_clock(start())
        .seed(1);
    for d in defs {
        b = b.sim(d);
    }
    (b.start().expect("engine starts"), log)
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
