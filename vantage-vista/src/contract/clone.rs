//! The clone contract: a clone of a Vista owns an independent copy of its
//! query.

use vantage_dataset::traits::ReadableValueSet;

use crate::{CborValue, FilterOp, Vista};

/// Asserts that:
/// - narrowing a clone by `column == value` leaves the original's
///   `list_values` and `query()` unchanged;
/// - the narrowed clone returns a subset, and records the clause;
/// - a clone taken after narrowing returns the narrowed set.
///
/// The fixture must pick a `column`/`value` pair that keeps some rows of
/// `vista` and drops others, so the narrowing is observable.
///
/// A shell that can't list (write-only, `list_values` answers `Unsupported`)
/// passes once the clone has narrowed.
pub async fn check_clone(vista: &Vista, column: &str, value: CborValue) {
    let query_before = vista.query().clone();
    let narrowed = vista
        .clone()
        .with_condition(column, FilterOp::Eq, value)
        .expect("with_condition on a clone must succeed");
    assert_eq!(
        narrowed.query().conditions.len(),
        query_before.conditions.len() + 1,
        "the narrowed clone must record its condition"
    );
    assert_eq!(
        vista.query(),
        &query_before,
        "narrowing a clone must not change the original's query()"
    );
    match vista.list_values().await {
        Err(e) if e.is_unsupported() => return,
        Err(e) => panic!("list_values on the original must succeed: {e}"),
        Ok(_) => {}
    }
    let before = ids(vista, "the original").await;
    let narrowed_ids = ids(&narrowed, "the narrowed clone").await;

    assert_eq!(
        ids(vista, "the original").await,
        before,
        "narrowing a clone must not change the original's list_values"
    );
    assert!(
        !narrowed_ids.is_empty() && narrowed_ids.len() < before.len(),
        "the fixture's condition must keep some rows and drop others \
         (original {}, narrowed {})",
        before.len(),
        narrowed_ids.len()
    );
    assert!(
        narrowed_ids.iter().all(|id| before.contains(id)),
        "the narrowed clone must return a subset of the original's rows"
    );

    let again = narrowed.clone();
    assert_eq!(
        ids(&again, "a clone of the narrowed clone").await,
        narrowed_ids,
        "a clone taken after narrowing must carry the narrowing"
    );
}

/// Sorted ids of every row `vista` lists.
async fn ids(vista: &Vista, what: &str) -> Vec<String> {
    let rows = vista
        .list_values()
        .await
        .unwrap_or_else(|e| panic!("list_values on {what} must succeed: {e}"));
    let mut ids: Vec<String> = rows.into_keys().collect();
    ids.sort();
    ids
}
