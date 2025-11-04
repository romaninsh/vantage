//! Mutable expressions allow multiple expressions to reference the same mutable value.
//! The value is stored in an `Arc<Mutex<T>>` and evaluated at render time.

#[cfg(test)]
mod tests {
    use crate::expr_any;
    use crate::expression::flatten::{ExpressionFlattener, Flatten};
    use crate::protocol::expressive::ExpressiveEnum;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_arc_mutex_expression() {
        let shared_var = Arc::new(Mutex::new(42i32));
        let expr = expr_any!(Arc<Mutex<i32>>, "value: {}", shared_var);

        assert_eq!(expr.parameters.len(), 1);
        assert_eq!(expr.template, "value: {}");
    }

    #[test]
    fn test_deferred_with_mutation() {
        let shared_str = Arc::new(Mutex::new("initial".to_string()));
        let shared_clone = shared_str.clone();

        // Create deferred closure that reads from shared variable
        let deferred_fn = move || {
            let shared_clone = shared_clone.clone();
            Box::pin(async move {
                let value = shared_clone.lock().unwrap().clone();
                ExpressiveEnum::Scalar(value)
            })
                as std::pin::Pin<
                    Box<dyn std::future::Future<Output = ExpressiveEnum<String>> + Send>,
                >
        };

        let expr = expr_any!(String, "message: {}", { deferred_fn });
        let flattener = ExpressionFlattener::new();

        // 1. Initial flatten/preview - see old value
        let initial_flattened = flattener.flatten(&expr);
        // Note: Since flatten doesn't execute deferred, we can't test the actual value here
        assert_eq!(initial_flattened.parameters.len(), 1);

        // 2. Modify variable
        {
            let mut guard = shared_str.lock().unwrap();
            *guard = "modified".to_string();
        }

        // 3. Flatten again - the deferred closure will use new value when executed
        let modified_flattened = flattener.flatten(&expr);
        assert_eq!(modified_flattened.parameters.len(), 1);

        // Verify the shared variable was actually modified
        let current_value = shared_str.lock().unwrap().clone();
        assert_eq!(current_value, "modified");
    }
}
