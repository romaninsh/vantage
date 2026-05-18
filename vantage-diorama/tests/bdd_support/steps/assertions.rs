//! Shared assertion steps — kept out of phase-specific files so future
//! phases (event_path, refresh) can reuse the snapshot infrastructure
//! without copy-paste.

use cucumber::then;

use crate::bdd_support::world::DioramaWorld;

#[then(regex = r#"^the event log matches snapshot "([^"]+)"$"#)]
async fn event_log_snapshot(w: &mut DioramaWorld, name: String) {
    // Materialise as Debug strings so we don't have to teach DioEvent how
    // to Serialize. The filter strips file:line:col tails that creep in
    // through `vantage_core::error!` so the snapshot is stable across
    // unrelated edits.
    let events: Vec<String> = w
        .snapshot_events()
        .await
        .iter()
        .map(|e| format!("{e:?}"))
        .collect();

    insta::with_settings!({
        filters => vec![
            (r#"[A-Za-z0-9_./\\-]+\.rs:\d+:\d+"#, "[LOC]"),
        ],
        snapshot_path => "../../snapshots",
        snapshot_suffix => name.as_str(),
        prepend_module_to_snapshot => false,
    }, {
        insta::assert_yaml_snapshot!(events);
    });
}
