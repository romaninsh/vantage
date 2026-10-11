//! A paged scenery's conditions must reach its chunk loader, and rows
//! outside them must not appear.
//!
//! Today `ChunkQuery` carries only the sort, the search and the operator
//! terms from `set_filter_terms`. Builder `where_eq` conditions and
//! `set_filters` pairs never reach the loader, and the paged path doesn't
//! apply them to the loaded rows either. In 2.0 the loader receives one
//! `VistaQuery` holding every condition; here the closest carrier is
//! `ChunkQuery::filters`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{ChunkQuery, Lens, TableScenery};
use vantage_vista::FilterOp;

use super::fixtures::{assert_no_failures, eventually};
use crate::support::chunk::{Backend, master};
use crate::support::{record, text};

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
fn recording_lens(backend: Backend, seen: Arc<Mutex<Vec<ChunkQuery>>>) -> Arc<Lens> {
    let lens = Lens::new()
        .cache_in_memory()
        .viewport_debounce(Duration::from_millis(1))
        .on_load_chunk(move |_dio, range, query, sink| {
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
        })
        .build()
        .expect("lens builds");
    Arc::new(lens)
}

/// The equality conditions a chunk query carries, as `(column, value)`.
/// The one place that reads `ChunkQuery`; when the loader receives a
/// `VistaQuery` instead, only this function changes.
fn eq_terms(query: &ChunkQuery) -> Vec<(String, CborValue)> {
    query
        .filters
        .iter()
        .filter(|term| term.op == FilterOp::Eq)
        .map(|term| (term.column.clone(), term.value.clone()))
        .collect()
}

/// Whether the most recent chunk query carried `column = value`.
fn last_query_has(seen: &Mutex<Vec<ChunkQuery>>, column: &str, value: &str) -> bool {
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
#[ignore = "v2 phase 3: paged loads drop where_eq and set_filters conditions"]
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
        .make_dio(master(&[("team", "String"), ("size", "String")]))
        .await?;
    let mut failures = Vec::new();

    let scenery = dio
        .table_scenery()
        .where_eq("team", text("red"))
        .page_size(10)
        .open()
        .await?;
    scenery.set_viewport(0..10);
    eventually(WAIT, || {
        last_query_has(&seen, "team", "red") && ids(&shown(&scenery)) == ["1", "3", "5"]
    })
    .await;
    if !last_query_has(&seen, "team", "red") {
        failures.push("the chunk loader never received where_eq(team = red)".to_string());
    }
    let rows = shown(&scenery);
    if ids(&rows) != ["1", "3", "5"] {
        failures.push(format!(
            "where_eq(team = red) view must show rows 1, 3, 5; shows {rows:?}"
        ));
    }

    scenery.set_filters(vec![("size".to_string(), text("big"))]);
    scenery.set_viewport(0..10);
    eventually(WAIT, || {
        last_query_has(&seen, "size", "big") && ids(&shown(&scenery)) == ["1", "5"]
    })
    .await;
    if !last_query_has(&seen, "size", "big") {
        failures.push("the chunk loader never received set_filters(size = big)".to_string());
    }
    let rows = shown(&scenery);
    if ids(&rows) != ["1", "5"] {
        failures.push(format!(
            "where_eq(team = red) + set_filters(size = big) view must show rows 1, 5; shows {rows:?}"
        ));
    }

    assert_no_failures(&failures);
    Ok(())
}
