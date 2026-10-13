//! SQL compares whole column values, so every dialect refuses a dotted path
//! into a column with `condition_column_error`, and `with_query` hands it back as
//! residual for the caller to evaluate.

use vantage_vista::{Condition, FilterOp, Vista, VistaQuery};

/// `path` names a field nested in a column of `vista`.
pub fn assert_dotted_path_is_refused(vista: &Vista, path: &str) {
    let err = vista
        .clone()
        .with_condition(path, FilterOp::Eq, "A")
        .unwrap_err();
    assert!(err.is_unimplemented(), "{err:?}");
    assert!(err.to_string().contains("dotted path"), "{err}");

    let condition = Condition::new(path, FilterOp::Eq, "A");
    let query = VistaQuery::new().with_condition(condition.clone());
    let (narrowed, residual) = vista.clone().with_query(&query).unwrap();
    assert_eq!(residual.conditions, vec![condition]);
    assert!(narrowed.query().conditions.is_empty());

    let unknown = vista
        .clone()
        .with_condition("no_such_column.leaf", FilterOp::Eq, "A")
        .unwrap_err();
    assert!(!unknown.is_unimplemented(), "{unknown:?}");
}
