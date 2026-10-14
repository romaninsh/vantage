//! A paged scenery's conditions must reach its chunk loader, and rows
//! outside them must not appear: the loader receives one `VistaQuery`
//! holding the builder's `filter` conditions and the `set_filters` ones.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Cache, Lens, Loader, TableScenery, Tuning};
use vantage_vista::{Condition, FilterOp, VistaQuery};

use super::fixtures::assert_no_failures;
use crate::support::chunk::{Backend, master};
use crate::support::{eventually, record, text};

/// Bounds each wait on the view; not a speed assertion.
const WAIT: Duration = Duration::from_secs(10);

/// `(id, team, size)`: two red-big rows among six.
const ROWS: &[(&str, &str, &str)] = &[
    ("1", "red", "big"),
    ("2", "blue", "big"),
    ("3", "red", "small"),
    ("4", "blue", "small"),
    ("5", "red", "big"),
    ("6", "blue", "big"),
];

/// A paged lens that serves `backend` filtered by the equality terms it is
/// handed, and records every query it receives.
fn recording_lens(backend: Backend, seen: Arc<Mutex<Vec<VistaQuery>>>) -> Arc<Lens> {
    let loader = Loader::paged(move |_cx, range, query, sink| {
        let backend = backend.clone();
        let seen = seen.clone();
        async move {
            let terms = eq_terms(&query);
            let rows: Vec<_> = backend
                .lock()
                .unwrap()
                .iter()
                .filter(|(_, row)| terms.iter().all(|(col, value)| row.get(col) == Some(value)))
                .cloned()
                .collect();
            seen.lock().unwrap().push(query);
            sink.set_total(rows.len());
            for idx in range {
                if let Some((id, row)) = rows.get(idx) {
                    sink.push(idx, id.clone(), row.clone()).await?;
                }
            }
            Ok(())
        }
    });
    Lens::builder()
        .cache(Cache::memory())
        .loader(loader)
        .tuning(Tuning::default().with_viewport_debounce(Duration::from_millis(1)))
        .build()
        .expect("lens builds")
}

/// The equality conditions a loader query carries, as `(column, value)`.
fn eq_terms(query: &VistaQuery) -> Vec<(String, CborValue)> {
    query
        .conditions
        .iter()
        .filter(|term| term.op == FilterOp::Eq)
        .map(|term| (term.column.clone(), term.value.clone()))
        .collect()
}

/// Whether the most recent loader query carried `column = value`.
fn last_query_has(seen: &Mutex<Vec<VistaQuery>>, column: &str, value: &str) -> bool {
    seen.lock()
        .unwrap()
        .last()
        .is_some_and(|query| eq_terms(query).contains(&(column.to_string(), text(value))))
}

/// `(id, team, size)` of every row the scenery shows.
fn shown(scenery: &Arc<dyn TableScenery>) -> Vec<(String, String, String)> {
    let field = |row: &vantage_types::Record<CborValue>, col: &str| match row.get(col) {
        Some(CborValue::Text(s)) => s.clone(),
        _ => String::new(),
    };
    (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .map(|row| {
            let r = &row.record;
            (field(r, "id"), field(r, "team"), field(r, "size"))
        })
        .collect()
}

fn ids(rows: &[(String, String, String)]) -> Vec<&str> {
    let mut ids: Vec<&str> = rows.iter().map(|(id, _, _)| id.as_str()).collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn paged_scenery_conditions() -> Result<()> {
    let backend: Backend = Arc::new(Mutex::new(
        ROWS.iter()
            .map(|(id, team, size)| {
                let row = record(&[("id", id), ("team", team), ("size", size)]);
                (id.to_string(), row)
            })
            .collect(),
    ));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let lens = recording_lens(backend, seen.clone());
    let dio = lens
        .dio(master(&[("team", "String"), ("size", "String")]))
        .build()
        .await?;
    let mut failures = Vec::new();

    let scenery = dio
        .table_scenery()
        .filter(Condition::new("team", FilterOp::Eq, "red"))
        .page_size(10)
        .open()
        .await?;
    scenery.set_viewport(0..10, 0..0);
    eventually(WAIT, || {
        last_query_has(&seen, "team", "red") && ids(&shown(&scenery)) == ["1", "3", "5"]
    })
    .await;
    if !last_query_has(&seen, "team", "red") {
        failures.push("the chunk loader never received filter(team = red)".to_string());
    }
    let rows = shown(&scenery);
    if ids(&rows) != ["1", "3", "5"] {
        failures.push(format!(
            "filter(team = red) view must show rows 1, 3, 5; shows {rows:?}"
        ));
    }

    scenery.set_filters(vec![Condition::new("size", FilterOp::Eq, "big")]);
    scenery.set_viewport(0..10, 0..0);
    eventually(WAIT, || {
        last_query_has(&seen, "size", "big") && ids(&shown(&scenery)) == ["1", "5"]
    })
    .await;
    if !last_query_has(&seen, "size", "big") {
        failures.push("the chunk loader never received set_filters(size = big)".to_string());
    }
    if !last_query_has(&seen, "team", "red") {
        failures.push("the chunk loader lost filter(team = red) after set_filters".to_string());
    }
    let rows = shown(&scenery);
    if ids(&rows) != ["1", "5"] {
        failures.push(format!(
            "filter(team = red) + set_filters(size = big) view must show rows 1, 5; shows {rows:?}"
        ));
    }

    assert_no_failures(&failures);
    Ok(())
}
