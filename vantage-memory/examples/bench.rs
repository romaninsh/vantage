//! Throughput at 1k, 10k and 100k rows. Run with:
//!
//!     cargo run -p vantage-memory --example bench --release
//!
//! Prints one line per operation per size. Nothing is asserted; this times
//! one process on one machine, so treat the numbers as relative, not
//! absolute.

use std::time::{Duration, Instant};

use ciborium::Value as CborValue;
use vantage_memory::{MemoryCondition, MemoryStore, Query, TableDef};
use vantage_types::Record;
use vantage_vista::{FilterOp, SortDirection};

const STATUSES: [&str; 5] = ["new", "open", "pending", "done", "cancelled"];

/// xorshift64: enough spread for sampling ids and statuses, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Rows per `group` value: an indexed column with about n/10 distinct
/// values, so an `Eq` on it selects ~10 rows at any size.
const GROUP_SIZE: usize = 10;

fn row(status: &str, n: i64) -> Record<CborValue> {
    [
        ("status".to_string(), CborValue::Text(status.to_string())),
        ("n".to_string(), CborValue::Integer(n.into())),
        (
            "group".to_string(),
            CborValue::Integer((n / GROUP_SIZE as i64).into()),
        ),
        ("name".to_string(), CborValue::Text(format!("row {n}"))),
    ]
    .into_iter()
    .collect()
}

/// Runs `f` `count` times and prints `size op ops/s µs/op`.
fn timed(size: usize, op: &str, count: usize, f: impl FnOnce()) {
    let start = Instant::now();
    f();
    report(size, op, count, start.elapsed());
}

fn report(size: usize, op: &str, count: usize, elapsed: Duration) {
    let secs = elapsed.as_secs_f64();
    let ops_per_sec = count as f64 / secs;
    let us_per_op = elapsed.as_micros() as f64 / count as f64;
    println!("{size:>7}  {op:<20}  {ops_per_sec:>12.0} ops/s  {us_per_op:>9.2} us/op");
}

fn bench_size(n: usize) {
    let store = MemoryStore::new();
    let table = store.define(
        "bench",
        TableDef {
            id_column: "id".to_string(),
            indexed: vec!["status".to_string(), "group".to_string()],
            id_prefix: None,
        },
    );
    let mut rng = Rng(0x9e3779b97f4a7c15 ^ n as u64);

    let mut ids = Vec::with_capacity(n);
    timed(n, "insert", n, || {
        for i in 0..n {
            let status = STATUSES[i % STATUSES.len()];
            ids.push(table.insert(row(status, i as i64)).unwrap());
        }
    });

    timed(n, "patch", n, || {
        for (i, id) in ids.iter().enumerate() {
            table.patch(
                id,
                &[("n".to_string(), CborValue::Integer((i as i64 + 1).into()))]
                    .into_iter()
                    .collect(),
            );
        }
    });

    timed(n, "get", n, || {
        for _ in 0..n {
            let id = &ids[rng.below(n as u64) as usize];
            table.get(id);
        }
    });

    let indexed_queries = 1000;
    timed(n, "indexed_eq", indexed_queries, || {
        for _ in 0..indexed_queries {
            let status = STATUSES[rng.below(STATUSES.len() as u64) as usize];
            let q = Query::new().filter(MemoryCondition::cmp(
                "status",
                FilterOp::Eq,
                CborValue::Text(status.to_string()),
            ));
            table.query(&q).unwrap();
        }
    });

    let groups = (n / GROUP_SIZE) as u64;
    timed(n, "indexed_eq_selective", indexed_queries, || {
        for _ in 0..indexed_queries {
            let q = Query::new().filter(MemoryCondition::cmp(
                "group",
                FilterOp::Eq,
                CborValue::Integer((rng.below(groups) as i64).into()),
            ));
            table.query(&q).unwrap();
        }
    });

    let range_queries = 100;
    timed(n, "range_scan", range_queries, || {
        for _ in 0..range_queries {
            let q = Query::new().filter(MemoryCondition::cmp(
                "n",
                FilterOp::Gt,
                CborValue::Integer(((n / 2) as i64).into()),
            ));
            table.query(&q).unwrap();
        }
    });

    let page_queries = 100;
    timed(n, "ordered_page", page_queries, || {
        for _ in 0..page_queries {
            let q = Query::new()
                .order_by("n", SortDirection::Descending)
                .window(n / 2, Some(50));
            table.query(&q).unwrap();
        }
    });

    // Deletes from the front: the worst case for the order-keeping row map.
    let deletes = (n / 10).min(1000);
    timed(n, "delete", deletes, || {
        for id in &ids[..deletes] {
            table.delete(id);
        }
    });
}

fn main() {
    for n in [1_000, 10_000, 100_000] {
        bench_size(n);
    }
}
