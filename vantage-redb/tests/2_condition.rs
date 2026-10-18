//! Test 2: RedbCondition construction (Eq, In) and Deferred resolution.
//!
//! Conditions are the public surface for filtering. `Eq` comes from
//! `column.eq(value)` (RedbOperation), `In` from `column.in_(values)`,
//! `Deferred` from `related_in_condition` and resolves to an `In` document
//! at execution time.

use ciborium::Value as CborValue;
use vantage_expressions::{DeferredFn, Expression, ExpressiveEnum};
use vantage_redb::condition::RedbCondition;
use vantage_redb::operation::RedbOperation;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;

fn deferred_returning(payload: CborValue) -> RedbCondition {
    RedbCondition::Deferred(DeferredFn::new(move || {
        let payload = payload.clone();
        Box::pin(async move { Ok(ExpressiveEnum::Scalar(AnyRedbType::untyped(payload))) })
    }))
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

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

#[test]
fn test_in_helper_builds_an_in_condition() {
    let cond = RedbCondition::in_("x", vec![1i64, 2]);

    assert_eq!(cond.column(), Some("x"));
    assert!(matches!(cond, RedbCondition::In { ref values, .. } if values.len() == 2));
}

#[test]
fn test_operations_work_on_any_expressive_value() {
    let expr: Expression<AnyRedbType> = Expression::new("owner", vec![]);

    assert_eq!(expr.eq("a").column(), Some("owner"));
    assert_eq!(expr.in_(vec!["a", "b"]).column(), Some("owner"));
}

// ── describe_condition / condition_column ─────────────────────────────────

#[test]
fn describe_condition_reads_as_the_expression() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let cond = RedbCondition::eq("parent", "p1");
    assert_eq!(db.describe_condition(&cond), "parent = \"p1\"");
}

#[test]
fn condition_column_names_eq_and_in() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let eq = RedbCondition::eq("parent", "p1");
    assert_eq!(db.condition_column(&eq), Some("parent".to_string()));
    let literal_in = RedbCondition::in_("invoice_id", vec!["i1", "i2"]);
    assert_eq!(
        db.condition_column(&literal_in),
        Some("invoice_id".to_string())
    );

    let invoices = Table::<Redb, EmptyEntity>::new("invoice", db.clone()).with_id_column("id");
    let in_cond = db.related_in_condition("invoice_id", &invoices, "id");
    assert_eq!(
        db.condition_column(&in_cond),
        Some("invoice_id".to_string())
    );
    assert_eq!(db.describe_condition(&in_cond), "invoice_id IN <deferred>");
}

#[test]
fn condition_column_names_an_in_values_condition() {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();
    let cond = db
        .in_values_condition("invoice_id", vec![AnyRedbType::new("i1".to_string())])
        .unwrap();
    assert_eq!(db.condition_column(&cond), Some("invoice_id".to_string()));
}

// ── Deferred resolution ───────────────────────────────────────────────────

#[tokio::test]
async fn test_deferred_resolves_to_in() {
    // The relationship traversal path encodes [target, [values]] as a
    // CBOR tuple wrapped in an untyped AnyRedbType. resolve() should
    // unpack that into an In condition.
    let cond = deferred_returning(CborValue::Array(vec![
        text("client_id"),
        CborValue::Array(vec![text("c1"), text("c2")]),
    ]));

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
    let cond = deferred_returning(CborValue::Array(vec![text("fk"), CborValue::Array(vec![])]));

    let resolved = cond.resolve().await.unwrap();

    match resolved {
        RedbCondition::In { values, .. } => assert!(values.is_empty()),
        _ => panic!("expected In"),
    }
}

#[tokio::test]
async fn test_deferred_has_no_column_until_resolved() {
    let cond = deferred_returning(CborValue::Array(vec![text("fk"), CborValue::Array(vec![])]));

    assert_eq!(cond.column(), None);
    assert_eq!(cond.resolve().await.unwrap().column(), Some("fk"));
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
async fn test_deferred_with_malformed_tuples_errors() {
    let wrong_length = CborValue::Array(vec![text("fk")]);
    let column_not_text =
        CborValue::Array(vec![CborValue::Integer(1.into()), CborValue::Array(vec![])]);
    let values_not_array = CborValue::Array(vec![text("fk"), text("c1")]);

    for payload in [wrong_length, column_not_text, values_not_array] {
        let result = deferred_returning(payload).resolve().await;
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn test_deferred_producing_a_non_scalar_errors() {
    let cond = RedbCondition::Deferred(DeferredFn::new(|| {
        Box::pin(async {
            Ok(ExpressiveEnum::Nested(Expression::<AnyRedbType>::new(
                "x",
                vec![],
            )))
        })
    }));

    assert!(cond.resolve().await.is_err());
}

#[tokio::test]
async fn test_resolve_passes_through_eq() {
    // Non-deferred conditions pass through resolve unchanged.
    let cond = RedbCondition::eq("a", 1i64);
    let resolved = cond.resolve().await.unwrap();
    match resolved {
        RedbCondition::Eq { column, value } => {
            assert_eq!(column, "a");
            assert_eq!(value.try_get::<i64>(), Some(1));
        }
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
