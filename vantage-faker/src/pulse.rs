//! Pulse sim — a generic, config-driven "live aggregate feed".
//!
//! Models an upstream stream-processing job (think a Kafka topic already
//! aggregated by a KTable): a fixed set of **keys** (categories — regions,
//! sites, shards…), each carrying a numeric value that drifts within a
//! rubber-banded ±`band_pct` of its baseline. It exposes a **coupled pair** of
//! tables from one shared run loop:
//!
//! - `Feed` (raw): an append log — every change is a new, newest-first row
//!   `{key_column, delta, updated}` that expires after `feed_retention`, so the
//!   stream visibly flows (the raw Kafka topic).
//! - `Aggregate` (derived): one keyed-upsert row per key — `{key_column,
//!   value_column, vs_baseline, live}`, the current value + its % deviation from
//!   baseline + liveness (the compacted KTable).
//! - `Minutes` (derived): an arrivals time series — one `{minute, attendees}`
//!   bucket per `bucket` window, summing only arrivals (positive deltas), kept
//!   to the last `minutes_window` buckets (oldest-first, so bars read L→R).
//!
//! Both are ordinary [`Vista`]s to the rest of Vantage; the sim broadcasts
//! `Inserted`/`Deleted` on the feed and `Updated` on the aggregate as values
//! move, which a subscribed Dio applies in place (no re-list). Every specific
//! (key names, baselines, rates, retention, which key blips offline) is config —
//! the crate stays domain-agnostic.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ciborium::Value as CborValue;
use fake::Fake;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::{Instant, interval};
use vantage_diorama::ChangeEvent;
use vantage_types::Record;
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

/// Broadcast backlog before a lagged subscriber must resync via `list`.
const EVENT_CAPACITY: usize = 1024;
/// How often the loop wakes to check per-key timers. Short, so the loop never
/// sleeps a whole interval — many keys due together produce a burst.
const LOOP_TICK: Duration = Duration::from_millis(150);
/// Offline-designated keys cycle: this long online, then this long offline.
const OFFLINE_ONLINE: Duration = Duration::from_secs(35);
const OFFLINE_OFFLINE: Duration = Duration::from_secs(16);

/// Which of the coupled tables a caller wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PulseRole {
    /// Raw append feed (newest-first, expiring): `{key_column, delta, updated}`.
    Feed,
    /// Derived per-key aggregate: `{key_column, value_column, vs_baseline, live}`.
    Aggregate,
    /// Per-bucket arrivals time series (oldest-first, rolling window):
    /// `{minute, attendees}`. Sums only *arriving* movement (positive deltas)
    /// within each `bucket`, so every bar is positive and the current one grows.
    Minutes,
}

/// One category the sim tracks, with the value it rubber-bands around.
#[derive(Clone, Debug)]
pub struct PulseKey {
    pub name: String,
    pub baseline: f64,
}

/// Everything the sim needs — all of it defined in YAML on the datasource.
#[derive(Clone, Debug)]
pub struct PulseConfig {
    /// Column holding the category name (e.g. `region`).
    pub key_column: String,
    /// Column holding the current value on the aggregate table (e.g. `visitors`).
    pub value_column: String,
    /// The categories + their baselines.
    pub keys: Vec<PulseKey>,
    /// Rubber-band half-width as a percentage of baseline (e.g. `5.0`).
    pub band_pct: f64,
    /// Per-key update cadence — each key re-fires at a random point in this range.
    pub min_interval: Duration,
    pub max_interval: Duration,
    /// How long a row lives on the `Feed` append log before it expires.
    pub feed_retention: Duration,
    /// Aggregation window for the `Minutes` arrivals series (one bar per bucket).
    pub bucket: Duration,
    /// How many `Minutes` buckets to keep (the rolling window width).
    pub minutes_window: usize,
    /// Keys that periodically blip offline (frozen value, `live = "Offline"`).
    pub offline: Vec<String>,
}

impl Default for PulseConfig {
    fn default() -> Self {
        Self {
            key_column: "key".into(),
            value_column: "value".into(),
            keys: Vec::new(),
            band_pct: 5.0,
            min_interval: Duration::from_secs(1),
            max_interval: Duration::from_secs(3),
            feed_retention: Duration::from_secs(10),
            bucket: Duration::from_secs(60),
            minutes_window: 10,
            offline: Vec::new(),
        }
    }
}

/// A running pulse sim. Cheap to [`Clone`] (shares the store, channels, and the
/// single run-loop task); the loop stops when the last clone is dropped.
#[derive(Clone)]
pub struct PulseSim {
    feed_shell: MockShell,
    agg_shell: MockShell,
    min_shell: MockShell,
    feed_tx: broadcast::Sender<ChangeEvent>,
    agg_tx: broadcast::Sender<ChangeEvent>,
    min_tx: broadcast::Sender<ChangeEvent>,
    key_column: String,
    value_column: String,
    _task: Arc<AbortOnDrop>,
}

impl PulseSim {
    /// Seed both tables at baseline and spawn the single mutation loop. Must be
    /// called inside a Tokio runtime context (the caller `enter()`s one).
    pub fn new(cfg: PulseConfig) -> Self {
        let feed_shell = MockShell::new();
        let agg_shell = MockShell::new();
        let min_shell = MockShell::new();
        let (feed_tx, _) = broadcast::channel(EVENT_CAPACITY);
        let (agg_tx, _) = broadcast::channel(EVENT_CAPACITY);
        let (min_tx, _) = broadcast::channel(EVENT_CAPACITY);

        // Seed the aggregate at baseline (no broadcast — no subscribers yet; the
        // lens snapshots this into its cache when the Dio is built). The feed
        // starts empty and fills as changes arrive.
        for k in &cfg.keys {
            agg_shell.set_record(
                &k.name,
                agg_record(
                    &cfg.key_column,
                    &k.name,
                    &cfg.value_column,
                    k.baseline,
                    0,
                    true,
                ),
            );
        }

        let task = spawn_loop(
            cfg.clone(),
            feed_shell.clone(),
            agg_shell.clone(),
            min_shell.clone(),
            feed_tx.clone(),
            agg_tx.clone(),
            min_tx.clone(),
        );

        Self {
            feed_shell,
            agg_shell,
            min_shell,
            feed_tx,
            agg_tx,
            min_tx,
            key_column: cfg.key_column,
            value_column: cfg.value_column,
            _task: Arc::new(AbortOnDrop(task)),
        }
    }

    /// A fresh [`Vista`] (named `name`) over the requested table's store, plus
    /// the delta [`broadcast::Sender`] to subscribe a forwarder to. Callable
    /// repeatedly — each call boxes a new shell clone sharing the same rows.
    pub fn table(
        &self,
        role: PulseRole,
        name: impl Into<String>,
    ) -> (Vista, broadcast::Sender<ChangeEvent>) {
        match role {
            PulseRole::Feed => (
                Vista::new(name, Box::new(self.feed_shell.clone())),
                self.feed_tx.clone(),
            ),
            PulseRole::Aggregate => (
                Vista::new(name, Box::new(self.agg_shell.clone())),
                self.agg_tx.clone(),
            ),
            PulseRole::Minutes => (
                Vista::new(name, Box::new(self.min_shell.clone())),
                self.min_tx.clone(),
            ),
        }
    }

    /// The aggregate table's value column name — handy for callers wiring sort.
    pub fn value_column(&self) -> &str {
        &self.value_column
    }

    /// The key column name.
    pub fn key_column(&self) -> &str {
        &self.key_column
    }
}

/// Per-key mutable state carried by the run loop.
struct KeyState {
    name: String,
    baseline: f64,
    current: f64,
    next_fire: Instant,
    designated_offline: bool,
    /// Phase offset so offline keys don't all blip in unison.
    phase: Duration,
}

#[allow(clippy::too_many_arguments)]
fn spawn_loop(
    cfg: PulseConfig,
    feed_shell: MockShell,
    agg_shell: MockShell,
    min_shell: MockShell,
    feed_tx: broadcast::Sender<ChangeEvent>,
    agg_tx: broadcast::Sender<ChangeEvent>,
    min_tx: broadcast::Sender<ChangeEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let start = Instant::now();
        let band = cfg.band_pct / 100.0;
        let (min_ms, max_ms) = (
            cfg.min_interval.as_millis() as u64,
            (cfg.max_interval.as_millis() as u64).max(cfg.min_interval.as_millis() as u64 + 1),
        );

        // Independent per-key timers, staggered across the first interval so the
        // opening burst spreads out.
        let mut keys: Vec<KeyState> = cfg
            .keys
            .iter()
            .enumerate()
            .map(|(i, k)| KeyState {
                name: k.name.clone(),
                baseline: k.baseline,
                current: k.baseline,
                next_fire: start + Duration::from_millis(rand_range(0, max_ms)),
                designated_offline: cfg.offline.iter().any(|o| o == &k.name),
                phase: Duration::from_secs((i as u64) * 5),
            })
            .collect();

        // Feed append log: monotonic sequence → reverse-monotonic id so the
        // cache's ascending key order surfaces newest-first (no explicit sort);
        // `pending` tracks each row's expiry (all share `feed_retention`, so it
        // stays time-ordered and a front-pop drains it).
        let mut seq: u64 = 0;
        let mut pending: VecDeque<(Instant, String)> = VecDeque::new();

        // Minutes arrivals series: bucket wall-clock time by `bucket`, sum only
        // arrivals (positive deltas) into the current bucket, keep the last
        // `minutes_window` buckets. Ascending id → oldest-first (left→right).
        let bucket_secs = cfg.bucket.as_secs().max(1);
        let window = cfg.minutes_window.max(1);
        let mut cur_bucket: u64 = u64::MAX;
        let mut cur_count: i64 = 0;
        let mut cur_label = String::new();
        let mut buckets: VecDeque<u64> = VecDeque::new();

        let mut ticker = interval(LOOP_TICK);
        loop {
            ticker.tick().await;
            let now = Instant::now();
            let elapsed = now.saturating_duration_since(start);

            // Roll the arrivals bucket over at each `bucket` boundary: open a new
            // bar at 0 and expire the oldest beyond the window.
            let epoch = epoch_secs();
            let bucket = epoch / bucket_secs;
            if bucket != cur_bucket {
                cur_bucket = bucket;
                cur_count = 0;
                cur_label = minute_label(epoch);
                let id = format!("{bucket:020}");
                let rec = minute_record(&cur_label, 0);
                min_shell.set_record(&id, rec.clone());
                let _ = min_tx.send(ChangeEvent::Inserted { id, new: Some(rec) });
                buckets.push_back(bucket);
                while buckets.len() > window {
                    let old = buckets.pop_front().expect("front present");
                    let oid = format!("{old:020}");
                    min_shell.remove_record(&oid);
                    let _ = min_tx.send(ChangeEvent::Deleted { id: oid });
                }
            }

            // Fire every key whose timer is due — a burst when several align.
            for k in keys.iter_mut() {
                if now < k.next_fire {
                    continue;
                }
                k.next_fire = now + Duration::from_millis(rand_range(min_ms, max_ms));

                let live = !(k.designated_offline
                    && is_offline(elapsed + k.phase, OFFLINE_ONLINE, OFFLINE_OFFLINE));

                if live {
                    let newv = rubber_band(k.current, k.baseline, band);
                    let delta = newv.round() as i64 - k.current.round() as i64;
                    k.current = newv;

                    // Append one row to the feed (a new change), newest-first.
                    seq += 1;
                    let fid = format!("{:020}", u64::MAX - seq);
                    let frec = feed_record(&cfg.key_column, &k.name, delta, &hhmmss());
                    feed_shell.set_record(&fid, frec.clone());
                    let _ = feed_tx.send(ChangeEvent::Inserted {
                        id: fid.clone(),
                        new: Some(frec),
                    });
                    pending.push_back((now + cfg.feed_retention, fid));

                    // Arrivals only: add the "up" movement to this minute's bar
                    // (ignore departures), so every bar is positive and grows.
                    if delta > 0 {
                        cur_count += delta;
                        let id = format!("{cur_bucket:020}");
                        let rec = minute_record(&cur_label, cur_count);
                        min_shell.set_record(&id, rec.clone());
                        let _ = min_tx.send(ChangeEvent::Updated { id, new: Some(rec) });
                    }
                }

                // Aggregate always reflects the current state (incl. the flip to
                // Offline); when offline the value stays frozen.
                let vs = vs_baseline(k.current, k.baseline);
                let rec = agg_record(
                    &cfg.key_column,
                    &k.name,
                    &cfg.value_column,
                    k.current,
                    vs,
                    live,
                );
                agg_shell.set_record(&k.name, rec.clone());
                let _ = agg_tx.send(ChangeEvent::Updated {
                    id: k.name.clone(),
                    new: Some(rec),
                });
            }

            // Expire feed rows past their retention (front-ordered by time).
            let exp_now = Instant::now();
            while let Some((at, _)) = pending.front() {
                if *at <= exp_now {
                    let (_, id) = pending.pop_front().expect("front present");
                    feed_shell.remove_record(&id);
                    let _ = feed_tx.send(ChangeEvent::Deleted { id });
                } else {
                    break;
                }
            }
        }
    })
}

/// Mean-reverting random step, clamped to ±`band` of `baseline`.
fn rubber_band(current: f64, baseline: f64, band: f64) -> f64 {
    let unit = rand_range(0, 2001) as f64 / 1000.0 - 1.0; // [-1, 1]
    let noise = unit * baseline * 0.02; // ±2% of baseline per tick
    let reversion = -(current - baseline) * 0.15; // pull back toward baseline
    let (lo, hi) = (baseline * (1.0 - band), baseline * (1.0 + band));
    (current + reversion + noise).clamp(lo, hi)
}

/// Percent deviation from baseline, rounded to a whole number.
fn vs_baseline(current: f64, baseline: f64) -> i64 {
    if baseline == 0.0 {
        return 0;
    }
    ((current - baseline) / baseline * 100.0).round() as i64
}

/// Where in a cycle of `online + offline` we are; `true` in the offline slice.
fn is_offline(t: Duration, online: Duration, offline: Duration) -> bool {
    let cycle = (online + offline).as_millis().max(1);
    let pos = t.as_millis() % cycle;
    pos >= online.as_millis()
}

fn agg_record(
    key_column: &str,
    name: &str,
    value_column: &str,
    value: f64,
    vs: i64,
    live: bool,
) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(key_column.to_string(), CborValue::Text(name.to_string()));
    r.insert(
        value_column.to_string(),
        CborValue::Integer((value.round() as i64).into()),
    );
    r.insert("vs_baseline".to_string(), CborValue::Integer(vs.into()));
    r.insert(
        "live".to_string(),
        CborValue::Text(if live { "Live" } else { "Offline" }.to_string()),
    );
    r
}

fn feed_record(key_column: &str, name: &str, delta: i64, updated: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(key_column.to_string(), CborValue::Text(name.to_string()));
    r.insert("delta".to_string(), CborValue::Integer(delta.into()));
    r.insert("updated".to_string(), CborValue::Text(updated.to_string()));
    r
}

fn minute_record(minute: &str, attendees: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("minute".to_string(), CborValue::Text(minute.to_string()));
    r.insert(
        "attendees".to_string(),
        CborValue::Integer(attendees.into()),
    );
    r
}

/// Seconds since the Unix epoch (0 if the clock is before it).
fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `:MM` minute-of-hour label for the arrivals x-axis (`:39`).
fn minute_label(epoch: u64) -> String {
    format!(":{:02}", (epoch / 60) % 60)
}

/// `[lo, hi)` — inclusive lo, exclusive hi. `hi` is bumped past `lo` if equal.
fn rand_range(lo: u64, hi: u64) -> u64 {
    (lo..hi.max(lo + 1)).fake()
}

/// Wall-clock `HH:MM:SS` (UTC), for the feed's `updated` column.
fn hhmmss() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

/// Aborts the run loop when the last [`PulseSim`] clone drops.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_dataset::prelude::ReadableValueSet;

    fn cfg() -> PulseConfig {
        PulseConfig {
            key_column: "region".into(),
            value_column: "visitors".into(),
            keys: vec![
                PulseKey {
                    name: "London".into(),
                    baseline: 5300.0,
                },
                PulseKey {
                    name: "Wales".into(),
                    baseline: 1100.0,
                },
            ],
            band_pct: 5.0,
            min_interval: Duration::from_millis(5),
            max_interval: Duration::from_millis(10),
            feed_retention: Duration::from_secs(10),
            bucket: Duration::from_secs(60),
            minutes_window: 10,
            offline: vec![],
        }
    }

    #[tokio::test]
    async fn seeds_aggregate_at_baseline_feed_empty() {
        let sim = PulseSim::new(PulseConfig {
            // No churn during the assertion window: fire far in the future.
            min_interval: Duration::from_secs(3600),
            max_interval: Duration::from_secs(7200),
            ..cfg()
        });
        let (feed, _) = sim.table(PulseRole::Feed, "updates");
        let (agg, _) = sim.table(PulseRole::Aggregate, "top");
        // Feed starts empty (fills as changes arrive); aggregate seeds one/key.
        assert_eq!(feed.list_values().await.unwrap().len(), 0);
        let agg_rows = agg.list_values().await.unwrap();
        assert_eq!(agg_rows.len(), 2);
        let london = &agg_rows["London"];
        assert_eq!(
            london.get("visitors"),
            Some(&CborValue::Integer(5300.into()))
        );
        assert_eq!(london.get("live"), Some(&CborValue::Text("Live".into())));
    }

    #[tokio::test]
    async fn feed_inserts_aggregate_updates() {
        let sim = PulseSim::new(cfg());
        let (_, feed_tx) = sim.table(PulseRole::Feed, "updates");
        let (_, agg_tx) = sim.table(PulseRole::Aggregate, "top");
        let mut frx = feed_tx.subscribe();
        let mut arx = agg_tx.subscribe();
        let got = tokio::time::timeout(Duration::from_secs(1), async {
            (
                recv_kind(&mut frx, "inserted").await,
                recv_kind(&mut arx, "updated").await,
            )
        })
        .await
        .expect("expected feed Inserted + aggregate Updated within 1s");
        assert!(got.0, "feed should broadcast Inserted");
        assert!(got.1, "aggregate should broadcast Updated");
    }

    #[tokio::test]
    async fn feed_accumulates_then_expires() {
        let sim = PulseSim::new(PulseConfig {
            min_interval: Duration::from_millis(5),
            max_interval: Duration::from_millis(10),
            feed_retention: Duration::from_millis(60),
            ..cfg()
        });
        let (feed, _) = sim.table(PulseRole::Feed, "updates");
        tokio::time::sleep(Duration::from_millis(250)).await;
        let n = feed.list_values().await.unwrap().len();
        // Flowing but bounded: rows accumulate and old ones expire, so the feed
        // stays well under "every event ever" (~50 at this rate) and non-empty.
        assert!(n > 0, "feed should have flowed rows");
        assert!(n < 40, "feed should be bounded by retention, got {n}");
    }

    #[tokio::test]
    async fn minutes_accumulate_arrivals_only_and_roll() {
        let sim = PulseSim::new(PulseConfig {
            min_interval: Duration::from_millis(5),
            max_interval: Duration::from_millis(10),
            bucket: Duration::from_millis(40),
            minutes_window: 4,
            ..cfg()
        });
        let (mins, _) = sim.table(PulseRole::Minutes, "minutes");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let rows = mins.list_values().await.unwrap();
        // Rolling window keeps at most `minutes_window` buckets.
        assert!(!rows.is_empty(), "minutes should have buckets");
        assert!(
            rows.len() <= 4,
            "window should cap buckets, got {}",
            rows.len()
        );
        // Arrivals-only → every attendees count is non-negative.
        for rec in rows.values() {
            let a = match rec.get("attendees") {
                Some(CborValue::Integer(i)) => i128::from(*i),
                other => panic!("attendees not int: {other:?}"),
            };
            assert!(a >= 0, "attendees must be arrivals-only (>= 0), got {a}");
        }
    }

    #[tokio::test]
    async fn value_stays_within_band() {
        let sim = PulseSim::new(cfg());
        let (agg, _) = sim.table(PulseRole::Aggregate, "top");
        // Let it churn.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let rows = agg.list_values().await.unwrap();
        for (name, base) in [("London", 5300i64), ("Wales", 1100)] {
            let v = match rows[name].get("visitors") {
                Some(CborValue::Integer(i)) => i128::from(*i) as i64,
                other => panic!("visitors not int: {other:?}"),
            };
            let dev = (v - base).abs() as f64 / base as f64;
            assert!(dev <= 0.051, "{name} drifted {dev} outside ±5%");
        }
    }

    #[test]
    fn rubber_band_never_leaves_the_band() {
        let baseline = 2000.0;
        let band = 0.05;
        let mut current = baseline;
        for _ in 0..10_000 {
            current = rubber_band(current, baseline, band);
            assert!(current >= baseline * 0.95 && current <= baseline * 1.05);
        }
    }

    #[test]
    fn offline_cycle_flips() {
        let online = Duration::from_secs(30);
        let offline = Duration::from_secs(10);
        assert!(!is_offline(Duration::from_secs(0), online, offline));
        assert!(!is_offline(Duration::from_secs(29), online, offline));
        assert!(is_offline(Duration::from_secs(31), online, offline));
        assert!(is_offline(Duration::from_secs(39), online, offline));
        assert!(!is_offline(Duration::from_secs(41), online, offline)); // wrapped to next cycle
    }

    async fn recv_kind(rx: &mut broadcast::Receiver<ChangeEvent>, kind: &str) -> bool {
        loop {
            match (rx.recv().await, kind) {
                (Ok(ChangeEvent::Inserted { .. }), "inserted") => return true,
                (Ok(ChangeEvent::Updated { .. }), "updated") => return true,
                (Ok(_), _) => continue,
                (Err(_), _) => return false,
            }
        }
    }
}
