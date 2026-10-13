//! The `notify` watcher delivers changed paths under the root.

#![cfg(feature = "watch")]

use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn a_written_file_reaches_the_callback() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("note")).unwrap();
    let root = dir.path().canonicalize().unwrap();
    let (tx, rx) = mpsc::channel();
    let _watcher = vantage_inventory::watch(&root, move |path| {
        if let Ok(path) = path {
            let _ = tx.send(path);
        }
    })
    .unwrap();

    let target = root.join("note/a.yaml");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();
    // Some backends need a moment after `watch` before they report; keep
    // rewriting until the event shows up or the deadline passes.
    while Instant::now() < deadline {
        std::fs::write(&target, "title: A\n").unwrap();
        if let Ok(path) = rx.recv_timeout(Duration::from_millis(250)) {
            if path == target {
                return;
            }
            seen.push(path);
        }
    }
    panic!("no event for {} within 5s; saw {seen:?}", target.display());
}
