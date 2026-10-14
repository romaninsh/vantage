//! Type-erased expressions: store anything, recover the concrete type by downcasting.

use std::any::TypeId;

use vantage_expressions::AnyExpression;

#[derive(Debug, Clone, PartialEq)]
struct Sample {
    value: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct Other {
    text: String,
}

#[test]
fn downcast_recovers_the_stored_value() {
    let any = AnyExpression::new(Sample { value: 42 });

    assert!(any.is_type::<Sample>());
    assert!(!any.is_type::<Other>());
    assert_eq!(any.type_name(), std::any::type_name::<Sample>());
    assert_eq!(any.type_id(), TypeId::of::<Sample>());
    assert_eq!(any.downcast::<Sample>().unwrap(), Sample { value: 42 });
}

#[test]
fn downcast_to_another_type_gives_the_expression_back() {
    let any = AnyExpression::new(Sample { value: 1 });

    let back = any.downcast::<Other>().unwrap_err();

    assert!(back.is_type::<Sample>());
    assert_eq!(back.downcast::<Sample>().unwrap(), Sample { value: 1 });
}

#[test]
fn downcast_ref_borrows_without_consuming() {
    let any = AnyExpression::new(Sample { value: 7 });

    assert_eq!(any.downcast_ref::<Sample>(), Some(&Sample { value: 7 }));
    assert_eq!(any.downcast_ref::<Other>(), None);
    assert!(any.is_type::<Sample>());
}

#[test]
fn clone_is_independent_and_keeps_the_type() {
    let any = AnyExpression::new(Other {
        text: "hi".to_string(),
    });
    let copy = any.clone();

    assert_eq!(copy.type_name(), any.type_name());
    assert_eq!(copy.type_id(), any.type_id());
    assert_eq!(
        copy.downcast::<Other>().unwrap(),
        any.downcast::<Other>().unwrap()
    );
}

#[test]
fn debug_names_the_wrapper_and_the_type() {
    let any = AnyExpression::new(Sample { value: 1 });

    let debug = format!("{:?}", any);

    assert!(debug.contains("AnyExpression"));
    assert!(debug.contains("Sample"));
}
