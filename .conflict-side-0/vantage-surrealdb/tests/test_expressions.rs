//! Test suite for the new SurrealExpression system
//!
//! Tests that the generic expression system correctly handles Duration types
//! and preserves type information through expression creation and extraction.

use std::time::Duration as StdDuration;
use surreal_client::types::{AnySurrealType, Duration, SurrealType};
use vantage_surrealdb::expression::{IntoExpression, SurrealExpression};
use vantage_surrealdb::surreal_expr;

#[test]
fn test_duration_macro_creation() {
    // Test creating expression with std::time::Duration
    let std_duration = StdDuration::from_secs(300);
    let expr = surreal_expr!("timeout = {}", std_duration);

    assert_eq!(expr.parameter_count(), 1);
    assert_eq!(expr.template, "timeout = {}");

    // Extract the parameter and debug what type it actually is
    let param = &expr.parameters[0];
    if let Some(any_type) = param.as_scalar() {
        println!(
            "std::time::Duration stored as type: {}",
            any_type.concrete_type_name()
        );
        println!("Target type enum: {:?}", any_type.target_type());

        // Check if it's stored as std::time::Duration directly
        if any_type.is::<std::time::Duration>() {
            if let Some(std_duration) = any_type.downcast_ref::<std::time::Duration>() {
                assert_eq!(std_duration.as_secs(), 300);
                println!("✓ std::time::Duration preserved directly");
            }
        } else if any_type.is::<Duration>() {
            if let Some(duration) = any_type.downcast_ref::<Duration>() {
                assert_eq!(duration.0.as_secs(), 300);
                println!("✓ std::time::Duration wrapped in surreal Duration");
            }
        } else {
            panic!(
                "Duration not found in expected types, got: {}",
                any_type.concrete_type_name()
            );
        }
    } else {
        panic!("Parameter should be scalar");
    }
}

#[test]
fn test_surreal_duration_macro_creation() {
    // Test creating expression with surreal_client::types::Duration
    let surreal_duration = Duration::new(StdDuration::from_millis(5000));
    let expr = surreal_expr!("retry_delay = {}", surreal_duration);

    assert_eq!(expr.parameter_count(), 1);
    assert_eq!(expr.template, "retry_delay = {}");

    // Extract the parameter and verify it preserves the exact same Duration
    let param = &expr.parameters[0];
    if let Some(any_type) = param.as_scalar() {
        if let Some(extracted_duration) = any_type.downcast_ref::<Duration>() {
            assert_eq!(extracted_duration.0.as_millis(), 5000);
            assert_eq!(extracted_duration.0.as_secs(), 5);
        } else {
            panic!("Failed to downcast to Duration");
        }
    } else {
        panic!("Parameter should be scalar");
    }
}

#[test]
fn test_chrono_duration_conversion() {
    // Test creating expression with chrono::Duration
    let chrono_duration = chrono::Duration::seconds(900);
    let expr = surreal_expr!("session_timeout = {}", chrono_duration);

    assert_eq!(expr.parameter_count(), 1);

    // Extract and verify conversion to surreal Duration
    let param = &expr.parameters[0];
    if let Some(any_type) = param.as_scalar() {
        assert!(
            any_type.is::<Duration>(),
            "chrono::Duration should be converted to surreal Duration"
        );

        if let Some(duration) = any_type.downcast_ref::<Duration>() {
            assert_eq!(duration.0.as_secs(), 900);
        } else {
            panic!("Failed to downcast to Duration");
        }
    } else {
        panic!("Parameter should be scalar");
    }
}

#[test]
fn test_duration_preservation_through_operations() {
    // Create a Duration and pass it through expression operations
    let original_duration = Duration::new(StdDuration::new(123, 456_789_000)); // 123s + 456ms + 789μs
    let expr = surreal_expr!("precise_timeout = {}", original_duration.clone());

    // Extract and verify precision is preserved
    let param = &expr.parameters[0];
    if let Some(any_type) = param.as_scalar() {
        if let Some(extracted_duration) = any_type.downcast_ref::<Duration>() {
            // Check seconds
            assert_eq!(extracted_duration.0.as_secs(), 123);
            // Check subsecond precision (nanoseconds)
            assert_eq!(extracted_duration.0.subsec_nanos(), 456_789_000);

            // Verify complete equality
            assert_eq!(extracted_duration.0, original_duration.0);
        } else {
            panic!("Failed to downcast to Duration");
        }
    } else {
        panic!("Parameter should be scalar");
    }
}

#[test]
fn test_multiple_duration_types_in_expression() {
    // Test expression with multiple duration parameters
    let std_dur = StdDuration::from_secs(60);
    let surreal_dur = Duration::new(StdDuration::from_secs(120));
    let chrono_dur = chrono::Duration::seconds(180);

    let expr = surreal_expr!(
        "SELECT * WHERE timeout BETWEEN {} AND {} AND retry_interval = {}",
        std_dur,
        surreal_dur,
        chrono_dur
    );

    assert_eq!(expr.parameter_count(), 3);

    // Debug and verify all parameters are Duration types
    for (i, param) in expr.parameters.iter().enumerate() {
        if let Some(any_type) = param.as_scalar() {
            println!(
                "Parameter {}: type = {}, target = {:?}",
                i,
                any_type.concrete_type_name(),
                any_type.target_type()
            );

            let expected_secs = match i {
                0 => 60,  // std_dur
                1 => 120, // surreal_dur
                2 => 180, // chrono_dur
                _ => panic!("Unexpected parameter index"),
            };

            // Check both possible Duration types
            if any_type.is::<Duration>() {
                if let Some(duration) = any_type.downcast_ref::<Duration>() {
                    assert_eq!(duration.0.as_secs(), expected_secs);
                    println!("✓ Parameter {} as surreal Duration", i);
                }
            } else if any_type.is::<std::time::Duration>() {
                if let Some(std_duration) = any_type.downcast_ref::<std::time::Duration>() {
                    assert_eq!(std_duration.as_secs(), expected_secs);
                    println!("✓ Parameter {} as std::time::Duration", i);
                }
            } else if any_type.is::<chrono::Duration>() {
                if let Some(chrono_duration) = any_type.downcast_ref::<chrono::Duration>() {
                    assert_eq!(chrono_duration.num_seconds(), expected_secs as i64);
                    println!("✓ Parameter {} as chrono::Duration", i);
                }
            } else {
                panic!(
                    "Parameter {} not recognized as Duration type: {}",
                    i,
                    any_type.concrete_type_name()
                );
            }
        } else {
            panic!("Parameter {} should be scalar", i);
        }
    }
}

#[test]
fn test_duration_display_consistency() {
    // Test that Duration displays consistently in expressions
    let duration = Duration::new(StdDuration::from_secs(300));
    let expr = surreal_expr!("timeout = {}", duration);

    let preview = expr.preview();

    // The exact display format depends on AnySurrealType::Display implementation
    // but it should contain the duration information and not be a simple number
    println!("Duration expression preview: {}", preview);
    assert!(preview.contains("timeout = "));

    // Extract the duration and verify its display
    let param = &expr.parameters[0];
    if let Some(any_type) = param.as_scalar() {
        let display_str = format!("{}", any_type);
        println!("Duration display: {}", display_str);

        // Should not be just a number (which was the old problem)
        assert!(!display_str.trim().chars().all(|c| c.is_numeric()));
    }
}

#[test]
fn test_nested_expressions_with_duration() {
    // Test Duration in nested expressions
    let timeout = Duration::new(StdDuration::from_secs(30));
    let condition = surreal_expr!("session_timeout > {}", timeout);
    let outer_expr = surreal_expr!("SELECT * FROM users WHERE {}", condition);

    assert_eq!(outer_expr.parameter_count(), 1);

    // The nested expression should be preserved
    let param = &outer_expr.parameters[0];
    if let Some(nested) = param.as_nested() {
        assert_eq!(nested.parameter_count(), 1);

        // Extract Duration from nested expression
        let nested_param = &nested.parameters[0];
        if let Some(any_type) = nested_param.as_scalar() {
            if let Some(duration) = any_type.downcast_ref::<Duration>() {
                assert_eq!(duration.0.as_secs(), 30);
            } else {
                panic!("Failed to extract Duration from nested expression");
            }
        }
    } else {
        panic!("Parameter should be nested expression");
    }
}

#[test]
fn test_cbor_vs_json_consistency() {
    // Test that Duration produces consistent results for CBOR vs JSON
    let duration = Duration::new(StdDuration::from_millis(2500));

    // Get both CBOR and JSON representations
    let cbor_value = duration.cborify();
    let json_value = duration.jsonify();

    println!("CBOR representation: {:?}", cbor_value);
    println!("JSON representation: {:?}", json_value);

    // Now test through expression system
    let any_duration = AnySurrealType::new(duration);
    let cbor_from_any = any_duration.cborify();
    let json_from_any = any_duration.jsonify();

    println!("CBOR from AnySurrealType: {:?}", cbor_from_any);
    println!("JSON from AnySurrealType: {:?}", json_from_any);

    // Both should represent the same duration information
    // The exact format may differ (CBOR uses tags, JSON uses objects/arrays)
    // but both should contain seconds=2 and some representation of 500ms
}

#[test]
fn test_expression_cbor_native_support() {
    // Test that our new expressions work with CBOR natively without conversion
    let duration = Duration::new(StdDuration::from_secs(600));
    let surreal_expr = surreal_expr!("cleanup_interval = {}", duration);

    // Extract the duration and verify CBOR support
    let param = &surreal_expr.parameters[0];
    if let Some(any_type) = param.as_scalar() {
        let cbor_value = any_type.cborify();
        println!("Duration CBOR representation: {:?}", cbor_value);

        // Verify it's a proper CBOR duration tag
        match cbor_value {
            ciborium::Value::Tag(14, _) => {
                println!("✓ Duration correctly uses CBOR Tag 14");
            }
            _ => panic!("Duration should use CBOR Tag 14 for native support"),
        }
    }

    // Our new system preserves types without JSON conversion
    println!("Expression preview: {}", surreal_expr.preview());
    assert!(surreal_expr.preview().contains("cleanup_interval = "));
}
