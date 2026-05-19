//! Phase-5c steps: select a backend for the master, run the facade
//! read surface against it. The same scenario outline runs against
//! Mock, CSV, and SQLite.

use ciborium::Value as CborValue;
use cucumber::{given, then};
use vantage_dataset::traits::ReadableValueSet;

use crate::bdd_support::{backend::BackendKind, world::DioramaWorld};

#[given(regex = r"^the backend is (mock|csv|sqlite)$")]
async fn select_backend(w: &mut DioramaWorld, kind: String) {
    w.backend = BackendKind::parse(&kind);
}

#[then(regex = r"^the facade lists (\d+) rows?$")]
async fn facade_list_count(w: &mut DioramaWorld, expected: u64) {
    let dio = w.dio.as_ref().expect("dio not created");
    let rows = dio.vista().list_values().await.expect("facade list");
    assert_eq!(
        rows.len() as u64,
        expected,
        "facade list: want {expected}, got {}",
        rows.len()
    );
}

#[then(regex = r#"^get_value "([^"]+)" returns title "([^"]+)"$"#)]
async fn get_value_returns_title(w: &mut DioramaWorld, id: String, expected: String) {
    let dio = w.dio.as_ref().expect("dio not created");
    let row = dio
        .vista()
        .get_value(&id)
        .await
        .expect("facade get_value")
        .unwrap_or_else(|| panic!("facade missing record {id}"));
    let got = row
        .get("title")
        .and_then(|v| match v {
            CborValue::Text(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default();
    assert_eq!(got, expected, "{id}.title: want {expected}, got {got}");
}

#[then(regex = r"^the facade count is (\d+)$")]
async fn facade_count(w: &mut DioramaWorld, expected: i64) {
    let dio = w.dio.as_ref().expect("dio not created");
    let got = dio.vista().get_count().await.expect("facade count");
    assert_eq!(got, expected, "facade count: want {expected}, got {got}");
}
