//! Test 2: RedbCondition construction (Eq, In) and Deferred resolution.
//!
//! Conditions are the public surface for filtering. `Eq` comes from
//! `column.eq(value)` (RedbOperation), `In` from `column.in_(values)`,
//! `Deferred` from `related_in_condition` and resolves to an `In` document
//! at execution time.

use ciborium::Value as CborValue;
use vantage_expressions::{DeferredFn, ExpressiveEnum};
use vantage_redb::condition::RedbCondition;
use vantage_redb::operation::RedbOperation;
use vantage_redb::AnyRedbType;
use vantage_table::column::core::Column;

// ── Eq ─────────────────────────────────────────────────────────────────────

#[test]
fn test_eq_from_column() {
    let c = Column::<String>::new("email");
    let cond = c.eq("alice@example.com");
    match cond {
        RedbCondition::Eq { column, value } => {
            assert_eq!(column, "email");
            assert_eq!(value.try_get::<String>(), Some("alice@example.com".into()));
        }
        _ => panic!("expected Eq"),
    }
}

#[test]
fn test_eq_from_column_name_helper() {
    // The bare constructor is also part of the public API.
    let cond = RedbCondition::eq("status", "active");
    match cond {
        RedbCondition::Eq { column, value } => {
            assert_eq!(column, "status");
            assert_eq!(value.try_get::<String>(), Some("active".into()));
        }
        _ => panic!("expected Eq"),
    }
}

#[test]
fn test_eq_with_int_value() {
    let c = Column::<i64>::new("age");
    let cond = c.eq(42i64);
    match cond {
        RedbCondition::Eq { column, value } => {
            assert_eq!(column, "age");
            assert_eq!(value.try_get::<i64>(), Some(42));
        }
        _ => panic!("expected Eq"),
    }
}

#[test]
fn test_eq_column_target_extracted() {
    // RedbCondition::column() returns the target field.
    let c = Column::<String>::new("name");
    let cond = c.eq("Alice");
    assert_eq!(cond.column(), Some("name"));
}

// ── In ─────────────────────────────────────────────────────────────────────

#[test]
fn test_in_from_column() {
    let c = Column::<String>::new("status");
    let cond = c.in_(vec!["active", "pending"]);
    match cond {
        RedbCondition::In { column, values } => {
            assert_eq!(column, "status");
            assert_eq!(values.len(), 2);
            assert_eq!(values[0].try_get::<String>(), Some("active".into()));
            assert_eq!(values[1].try_get::<String>(), Some("pending".into()));
        }
        _ => panic!("expected In"),
    }
}

#[test]
fn test_in_empty_values() {
    let c = Column::<String>::new("status");
    let cond = c.in_(Vec::<String>::new());
    match cond {
        RedbCondition::In { column, values } => {
            assert_eq!(column, "status");
            assert_eq!(values.len(), 0);
        }
        _ => panic!("expected In"),
    }
}

#[test]
fn test_in_with_int_values() {
    let c = Column::<i64>::new("priority");
    let cond = c.in_(vec![1i64, 2, 3]);
    match cond {
        RedbCondition::In { column, values } => {
            assert_eq!(column, "priority");
            assert_eq!(values.len(), 3);
            assert_eq!(values[2].try_get::<i64>(), Some(3));
        }
        _ => panic!("expected In"),
    }
}

#[test]
fn test_in_column_target_extracted() {
    let c = Column::<String>::new("status");
    let cond = c.in_(vec!["a", "b"]);
    assert_eq!(cond.column(), Some("status"));
}

// ── Deferred resolution ───────────────────────────────────────────────────

#[tokio::test]
async fn test_deferred_resolves_to_in() {
    // The relationship traversal path encodes [target, [values]] as a
    // CBOR tuple wrapped in an untyped AnyRedbType. resolve() should
    // unpack that into an In condition.
    let payload = CborValue::Array(vec![
        CborValue::Text("client_id".into()),
        CborValue::Array(vec![
            CborValue::Text("c1".into()),
            CborValue::Text("c2".into()),
        ]),
    ]);

    let deferred = DeferredFn::new(move || {
        let payload = payload.clone();
        Box::pin(async move {
            Ok(ExpressiveEnum::Scalar(AnyRedbType::untyped(payload)))
        })
    });

    let cond = RedbCondition::Deferred(deferred);
    let resolved = cond.resolve().await.unwrap();

    match resolved {
        RedbCondition::In { column, values } => {
            assert_eq!(column, "client_id");
            assert_eq!(values.len(), 2);
            assert_eq!(values[0].try_get::<String>(), Some("c1".into()));
            assert_eq!(values[1].try_get::<String>(), Some("c2".into()));
        }
        _ => panic!("expected In after resolve"),
    }
}

#[tokio::test]
async fn test_deferred_with_empty_values() {
    let payload = CborValue::Array(vec![
        CborValue::Text("fk".into()),
        CborValue::Array(vec![]),
    ]);
    let deferred = DeferredFn::new(move || {
        let payload = payload.clone();
        Box::pin(async move {
            Ok(ExpressiveEnum::Scalar(AnyRedbType::untyped(payload)))
        })
    });
    let resolved = RedbCondition::Deferred(deferred).resolve().await.unwrap();
    match resolved {
        RedbCondition::In { values, .. } => assert!(values.is_empty()),
        _ => panic!("expected In"),
    }
}

#[tokio::test]
async fn test_deferred_bad_payload_errors() {
    // Non-tuple payload — resolve should error rather than silently coerce.
    let deferred = DeferredFn::new(move || {
        Box::pin(async move {
            Ok(ExpressiveEnum::Scalar(AnyRedbType::untyped(
                CborValue::Text("not-a-tuple".into()),
            )))
        })
    });
    let result = RedbCondition::Deferred(deferred).resolve().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resolve_passes_through_eq() {
    // Non-deferred conditions pass through resolve unchanged.
    let cond = RedbCondition::eq("a", 1i64);
    let resolved = cond.resolve().await.unwrap();
    match resolved {
        RedbCondition::Eq { column, .. } => assert_eq!(column, "a"),
        _ => panic!("expected Eq passthrough"),
    }
}

#[tokio::test]
async fn test_resolve_passes_through_in() {
    let cond = RedbCondition::in_("x", vec![1i64, 2]);
    let resolved = cond.resolve().await.unwrap();
    match resolved {
        RedbCondition::In { values, .. } => assert_eq!(values.len(), 2),
        _ => panic!("expected In passthrough"),
    }
}
