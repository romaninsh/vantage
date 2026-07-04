//! In-memory folder tree + the per-second simulation step.
//!
//! The tree is a single recursive [`Entry`] rooted at `""` (the top-level
//! "dates" folder). Files and folders share one struct so a listing views
//! both uniformly; folders carry a `children` map, files have `size`.
//!
//! [`simulate_second`] runs one virtual second of the model: bumps the
//! active access chunk (rolling when it crosses the threshold), rolls
//! error_logs at `error_pct_per_sec`, and rolls each event type at its
//! configured percent. [`Tree::touch`] propagates the new `modified` time
//! up the ancestor chain so a parent folder reflects any child change.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use fake::Fake;

use super::LiveFolderConfig;

/// File vs folder discriminator for [`Entry::kind`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Folder,
}

/// One node in the folder tree. Files have `size` and empty `children`;
/// folders have `size = 0` (computed on demand for the size vista) and a
/// named-children map. Both carry `created`/`modified`.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
    pub created: SystemTime,
    pub modified: SystemTime,
    pub children: BTreeMap<String, Entry>,
}

impl Entry {
    pub fn folder(name: String, now: SystemTime) -> Self {
        Self {
            name,
            kind: EntryKind::Folder,
            size: 0,
            created: now,
            modified: now,
            children: BTreeMap::new(),
        }
    }

    pub fn file(name: String, size: u64, now: SystemTime) -> Self {
        Self {
            name,
            kind: EntryKind::File,
            size,
            created: now,
            modified: now,
            children: BTreeMap::new(),
        }
    }
}

/// The full simulated tree, rooted at `""`.
#[derive(Debug)]
pub struct Tree {
    pub root: Entry,
}

impl Tree {
    pub fn new(now: SystemTime) -> Self {
        Self {
            root: Entry::folder(String::new(), now),
        }
    }

    /// Read-only lookup by `/`-separated path. Empty path → root.
    pub fn get(&self, path: &str) -> Option<&Entry> {
        if path.is_empty() {
            return Some(&self.root);
        }
        let mut cur = &self.root;
        for seg in path.split('/') {
            cur = cur.children.get(seg)?;
        }
        Some(cur)
    }

    /// Mutable lookup by `/`-separated path.
    pub fn get_mut(&mut self, path: &str) -> Option<&mut Entry> {
        if path.is_empty() {
            return Some(&mut self.root);
        }
        let mut cur = &mut self.root;
        for seg in path.split('/') {
            cur = cur.children.get_mut(seg)?;
        }
        Some(cur)
    }

    /// Propagate `when` to `modified` on every ancestor of `path` (and the
    /// leaf itself). The root always touches. Used after any leaf mutation
    /// so folder `modified` reflects its most-recently-changed child.
    pub fn touch(&mut self, path: &str, when: SystemTime) {
        self.root.modified = when;
        if path.is_empty() {
            return;
        }
        let mut cur = &mut self.root;
        for seg in path.split('/') {
            cur.modified = when;
            cur = match cur.children.get_mut(seg) {
                Some(c) => c,
                None => return,
            };
        }
        cur.modified = when;
    }
}

/// Run one virtual second of the simulation at time `when`.
///
/// The clock decides everything — date/hour/second all derive from `when`,
/// so calling this with the same `when` always lands mutations in the same
/// folders (random draws still vary, of course).
pub fn simulate_second(tree: &mut Tree, cfg: &LiveFolderConfig, when: SystemTime) {
    let (date_str, hour, hms) = split_time(when);

    // ---- access_logs: bump active chunk, roll when over threshold -----------
    let access_folder_name = format!("access_logs_{hour:02}");
    let access_folder_path = format!("{date_str}/{access_folder_name}");

    ensure_path(tree, &[&date_str, &access_folder_name], when);
    let line_len = rand_range(cfg.bytes_per_request.0, cfg.bytes_per_request.1 + 1);
    let delta = line_len * cfg.requests_per_sec;
    let chunk_name = bump_active_chunk(tree, &access_folder_path, cfg.chunk_threshold, delta, when);
    tree.touch(&format!("{access_folder_path}/{chunk_name}"), when);

    // ---- error_logs: probability-gated file creation -----------------------
    // 0.01%-precision roll so fractional percents (e.g. 0.1) work.
    let roll: f64 = (0..10_000).fake::<u64>() as f64 / 100.0;
    if roll < cfg.error_pct_per_sec {
        let err_folder_path = format!("{date_str}/error_logs");
        let err_file = format!("{hms}-errors.log");
        let err_size = rand_range(cfg.error_size.0, cfg.error_size.1 + 1);
        ensure_path(tree, &[&date_str, "error_logs"], when);
        if let Some(folder) = tree.get_mut(&err_folder_path) {
            folder.children.insert(
                err_file.clone(),
                Entry::file(err_file.clone(), err_size, when),
            );
        }
        tree.touch(&format!("{err_folder_path}/{err_file}"), when);
    }

    // ---- events: per-type probability-gated size bump ----------------------
    for (etype, pct) in super::EVENT_TYPES {
        if rand_range(0, 100) < *pct as u64 {
            let ev_folder_path = format!("{date_str}/events");
            let file_name = format!("{etype}.log");
            let bump = rand_range(cfg.event_bump.0, cfg.event_bump.1 + 1);
            ensure_path(tree, &[&date_str, "events"], when);
            if let Some(folder) = tree.get_mut(&ev_folder_path) {
                let file = folder
                    .children
                    .entry(file_name.clone())
                    .or_insert_with(|| Entry::file(file_name.clone(), 0, when));
                file.size += bump;
                file.modified = when;
            }
            tree.touch(&format!("{ev_folder_path}/{file_name}"), when);
        }
    }
}

/// Walk a path of segments, creating folder entries as needed.
fn ensure_path(tree: &mut Tree, segments: &[&str], now: SystemTime) {
    let mut cur = &mut tree.root;
    for seg in segments {
        cur = cur
            .children
            .entry((*seg).to_string())
            .or_insert_with(|| Entry::folder((*seg).to_string(), now));
    }
}

/// Add `delta` bytes to the active (highest-indexed) `chunk_NN.log` under
/// `folder_path`, creating a new chunk if the active one is over `threshold`.
/// When the active chunk's size crosses `threshold` after the bump, the next
/// chunk file is pre-created (size 0) so a listing shows it opening right
/// away. Returns the chunk name that received the bump.
fn bump_active_chunk(
    tree: &mut Tree,
    folder_path: &str,
    threshold: u64,
    delta: u64,
    now: SystemTime,
) -> String {
    let Some(folder) = tree.get_mut(folder_path) else {
        return String::new();
    };
    let max_idx = folder
        .children
        .keys()
        .filter_map(|k| {
            k.strip_prefix("chunk_")
                .and_then(|s| s.strip_suffix(".log"))
                .and_then(|s| s.parse::<usize>().ok())
        })
        .max();

    let idx = match max_idx {
        None => 1,
        Some(i) => {
            let name = format!("chunk_{i:02}.log");
            let size = folder.children.get(&name).map(|e| e.size).unwrap_or(0);
            if size < threshold { i } else { i + 1 }
        }
    };

    let name = format!("chunk_{idx:02}.log");
    let chunk = folder
        .children
        .entry(name.clone())
        .or_insert_with(|| Entry::file(name.clone(), 0, now));
    chunk.size += delta;
    chunk.modified = now;

    // Active chunk crossed threshold — open the next one (size 0) so a
    // listing sees the new chunk file appear in the same tick.
    if chunk.size >= threshold {
        let next_name = format!("chunk_{:02}.log", idx + 1);
        folder
            .children
            .entry(next_name.clone())
            .or_insert_with(|| Entry::file(next_name, 0, now));
    }

    name
}

/// Recursively sum file sizes under `entry`. Returns `(total_bytes, file_count)`.
pub fn folder_size(entry: &Entry) -> (u64, u64) {
    let mut size = 0u64;
    let mut files = 0u64;
    fn walk(e: &Entry, size: &mut u64, files: &mut u64) {
        match e.kind {
            EntryKind::File => {
                *size += e.size;
                *files += 1;
            }
            EntryKind::Folder => {
                for c in e.children.values() {
                    walk(c, size, files);
                }
            }
        }
    }
    walk(entry, &mut size, &mut files);
    (size, files)
}

// ---- time helpers (no chrono dep) ----------------------------------------

/// Split a SystemTime into `(date "YYYY-MM-DD", hour 0..=23, "HH:MM:SS")` UTC.
fn split_time(t: SystemTime) -> (String, u32, String) {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = (secs % 60) as u32;
    let m = ((secs / 60) % 60) as u32;
    let h = ((secs / 3600) % 24) as u32;
    let days = secs / 86400;
    let (y, mo, d) = days_to_ymd(days);
    (
        format!("{y:04}-{mo:02}-{d:02}"),
        h,
        format!("{h:02}:{m:02}:{s:02}"),
    )
}

/// `SystemTime` → "YYYY-MM-DD HH:MM:SS" UTC for record columns.
pub fn format_ts(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let s = (secs % 60) as u32;
    let m = ((secs / 60) % 60) as u32;
    let h = ((secs / 3600) % 24) as u32;
    let days = secs / 86400;
    let (y, mo, d) = days_to_ymd(days);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

/// Civil-from-days algorithm (Howard Hinnant). `days` is days since
/// 1970-01-01. Returns `(year, month 1..=12, day 1..=31)`.
fn days_to_ymd(days: u64) -> (u32, u32, u32) {
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

/// `[lo, hi)` random u64. Clamps to `lo` when `hi <= lo`.
pub fn rand_range(lo: u64, hi: u64) -> u64 {
    if hi <= lo {
        return lo;
    }
    (lo..hi).fake()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cfg() -> LiveFolderConfig {
        LiveFolderConfig {
            requests_per_sec: 100,
            bytes_per_request: (60, 100),
            chunk_threshold: 288_000,
            error_pct_per_sec: 100.0, // force every second
            error_size: (500, 500),
            event_bump: (2000, 2000),
            backfill: Duration::ZERO,
        }
    }

    #[test]
    fn touch_propagates_modified_up_to_root() {
        let mut tree = Tree::new(SystemTime::UNIX_EPOCH);
        ensure_path(
            &mut tree,
            &["2026-06-03", "access_logs_11"],
            SystemTime::UNIX_EPOCH,
        );
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        tree.touch("2026-06-03/access_logs_11/chunk_01.log", later);
        assert_eq!(tree.root.modified, later);
        assert_eq!(tree.get("2026-06-03").unwrap().modified, later);
        assert_eq!(
            tree.get("2026-06-03/access_logs_11").unwrap().modified,
            later
        );
    }

    #[test]
    fn chunk_rolls_when_threshold_met() {
        let mut tree = Tree::new(SystemTime::UNIX_EPOCH);
        let mut c = cfg();
        c.chunk_threshold = 200; // roll fast
        c.requests_per_sec = 1;
        c.bytes_per_request = (100, 100); // 100 bytes/sec

        // First second: chunk_01 created, size 100.
        simulate_second(&mut tree, &c, SystemTime::UNIX_EPOCH);
        let f = tree.get("1970-01-01/access_logs_00").unwrap();
        assert!(f.children.contains_key("chunk_01.log"));
        assert_eq!(f.children["chunk_01.log"].size, 100);

        // Second second: chunk_01 size 200, threshold met → chunk_02 created.
        simulate_second(
            &mut tree,
            &c,
            SystemTime::UNIX_EPOCH + Duration::from_secs(1),
        );
        let f = tree.get("1970-01-01/access_logs_00").unwrap();
        assert_eq!(f.children["chunk_01.log"].size, 200);
        assert!(f.children.contains_key("chunk_02.log"));
        assert_eq!(f.children["chunk_02.log"].size, 0);
    }

    #[test]
    fn folder_size_walks_recursively() {
        let now = SystemTime::UNIX_EPOCH;
        let mut tree = Tree::new(now);
        ensure_path(&mut tree, &["d", "access_logs_00"], now);
        let f = tree.get_mut("d/access_logs_00").unwrap();
        f.children.insert(
            "chunk_01.log".into(),
            Entry::file("chunk_01.log".into(), 100, now),
        );
        f.children.insert(
            "chunk_02.log".into(),
            Entry::file("chunk_02.log".into(), 250, now),
        );

        let (size, files) = folder_size(tree.get("d").unwrap());
        assert_eq!(size, 350);
        assert_eq!(files, 2);
    }
}
