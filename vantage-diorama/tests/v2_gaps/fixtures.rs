//! Fixtures shared by the gap tests.

use std::sync::Arc;

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
