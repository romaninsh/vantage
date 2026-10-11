//! The clone contract: `clone_shell` yields an independent copy of the query.

use vantage_dataset::traits::ReadableValueSet;

use crate::{CborValue, Vista};

/// Asserts that:
/// - the shell can be cloned (`clone_shell()` is `Some`);
/// - narrowing a clone by `column == value` leaves the original's
///   `list_values` unchanged;
/// - a clone taken after narrowing returns the narrowed set.
///
/// The fixture must pick a `column`/`value` pair that keeps some rows of
/// `vista` and drops others, so the narrowing is observable.
///
/// A shell that can't list (write-only, `list_values` answers `Unsupported`)
/// only has to be cloneable; phase 1 replaces this with a capability check.
pub async fn check_clone(vista: &Vista, column: &str, value: CborValue) {
    let mut narrowed = clone_of(vista, "the original");
    match vista.list_values().await {
        Err(e) if e.is_unsupported() => return,
        Err(e) => panic!("list_values on the original must succeed: {e}"),
        Ok(_) => {}
    }
    let before = ids(vista, "the original").await;

    narrowed
        .add_condition_eq(column, value)
        .expect("add_condition_eq on the clone must succeed");
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

    let again = clone_of(&narrowed, "the narrowed clone");
    assert_eq!(
        ids(&again, "a clone of the narrowed clone").await,
        narrowed_ids,
        "a clone taken after narrowing must carry the narrowing"
    );
}

fn clone_of(vista: &Vista, what: &str) -> Vista {
    let shell = vista
        .source
        .clone_shell()
        .unwrap_or_else(|| panic!("clone_shell() on {what} returned None: shell not cloneable"));
    Vista::new(vista.name(), shell)
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
