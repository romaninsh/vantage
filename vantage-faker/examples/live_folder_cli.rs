//! Watch a live log-folder tree in the terminal.
//!
//! Runs a [`LiveFolderSim`] with a 2-hour backfill, then prints the whole
//! tree each second — file/folder name, size, created, modified. The sim
//! ticks in real time, so new chunks appear, error files pop in, and event
//! files grow as you watch.
//!
//! ```sh
//! cargo run --example live_folder_cli
//! ```
//! Ctrl-C to quit.

use std::time::Duration;

use vantage_faker::{LiveFolderConfig, LiveFolderSim};

fn human_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n:>5} B ")
    } else if n < 1024 * 1024 {
        format!("{:5.1} KB", n as f64 / 1024.0)
    } else if n < 1024 * 1024 * 1024 {
        format!("{:5.1} MB", n as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:5.1} GB", n as f64 / 1024.0 / 1024.0 / 1024.0)
    }
}

/// Print the tree, indenting children. Files get `size + dates`; folders
/// just get a `modified` stamp (size is fetched via the size vista, not
/// shown here).
fn render(entries: &[(String, vantage_faker::live_folder::Entry)]) {
    use vantage_faker::live_folder::EntryKind;

    // Group by parent so we can render hierarchy.
    let mut by_parent: std::collections::BTreeMap<
        String,
        Vec<(String, vantage_faker::live_folder::Entry)>,
    > = std::collections::BTreeMap::new();
    for (path, entry) in entries {
        let parent = match path.rsplit_once('/') {
            Some((p, _)) => p.to_string(),
            None => String::new(),
        };
        by_parent
            .entry(parent)
            .or_default()
            .push((path.clone(), entry.clone()));
    }

    fn walk(
        prefix: &str,
        path: &str,
        by_parent: &std::collections::BTreeMap<
            String,
            Vec<(String, vantage_faker::live_folder::Entry)>,
        >,
    ) {
        let Some(children) = by_parent.get(path) else {
            return;
        };
        let last_idx = children.len().saturating_sub(1);
        for (i, (child_path, entry)) in children.iter().enumerate() {
            let is_last = i == last_idx;
            let branch = if is_last { "└── " } else { "├── " };
            let cont = if is_last { "    " } else { "│   " };

            let modified = entry.modified;
            let m_full = vantage_faker::live_folder::format_ts(modified);
            let m_str = m_full.split(' ').nth(1).unwrap_or("");
            let c_full = vantage_faker::live_folder::format_ts(entry.created);
            let c_str = c_full.split(' ').nth(1).unwrap_or("");

            match entry.kind {
                EntryKind::File => {
                    println!(
                        "{prefix}{branch}{:<28} {:<10}  mod {m_str}  created {c_str}",
                        entry.name,
                        human_bytes(entry.size)
                    );
                }
                EntryKind::Folder => {
                    println!("{prefix}{branch}{}/  mod {m_str}", entry.name);
                    let new_prefix = format!("{prefix}{cont}");
                    walk(&new_prefix, child_path, by_parent);
                }
            }
        }
    }

    println!(
        "\nvantage-faker · live folder tree (5 GB chunks, 0.1% error rate, 2h backfill) · mod = modified UTC · Ctrl-C to quit\n"
    );
    walk("", "", &by_parent);
}

#[tokio::main]
async fn main() {
    // 2-hour backfill so the tree opens already populated.
    let cfg = LiveFolderConfig {
        backfill: Duration::from_secs(2 * 3600),
        chunk_threshold: 5 * 1024 * 1024 * 1024, // 5 GB chunks
        error_pct_per_sec: 0.1,                  // 0.1% per second
        ..LiveFolderConfig::default()
    };
    let sim = LiveFolderSim::new(cfg);

    loop {
        // Clear screen + home cursor.
        print!("\x1B[2J\x1B[H");
        let snap = sim.snapshot();
        render(&snap);

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
