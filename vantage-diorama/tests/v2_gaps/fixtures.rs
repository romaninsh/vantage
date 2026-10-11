//! Fixtures shared by the gap tests.

use std::sync::Arc;
use std::time::Duration;

use vantage_diorama::TableScenery;

/// Ids of the rows a table scenery currently shows, sorted.
pub fn scenery_ids(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    let mut ids: Vec<String> = (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|row| match row.record.get("id") {
            Some(ciborium::Value::Text(id)) => Some(id.clone()),
            _ => None,
        })
        .collect();
    ids.sort();
    ids
}

/// Poll `holds` every 5ms until it is true or `within` elapses. Returns
/// whether it came true. Bounds a wait; it is not a speed assertion.
pub async fn eventually(within: Duration, holds: impl Fn() -> bool) -> bool {
    tokio::time::timeout(within, async {
        while !holds() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .is_ok()
}

/// Panic with every failed check at once, so a gap test that covers several
/// paths reports all the paths that are still wrong, not only the first.
pub fn assert_no_failures(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} check(s) failed:\n  - {}",
        failures.len(),
        failures.join("\n  - ")
    );
}
