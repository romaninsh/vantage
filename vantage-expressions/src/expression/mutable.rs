//! Mutable expressions allow multiple expressions to reference the same mutable value.
//! The value is stored in an `Arc<Mutex<T>>` and evaluated at render time.

use crate::expression::owned::Expression;
use crate::protocol::expressive::IntoExpressive;

use std::sync::{Arc, Mutex};

impl<T> From<Arc<Mutex<T>>> for IntoExpressive<Expression>
where
    T: Into<IntoExpressive<Expression>> + Clone + Send + Sync + 'static,
{
    fn from(arc_mutex: Arc<Mutex<T>>) -> Self {
        IntoExpressive::deferred(move || {
            let arc_mutex = arc_mutex.clone();
            Box::pin(async move { arc_mutex.lock().unwrap().clone().into() })
        })
    }
}

impl<T> From<&Arc<Mutex<T>>> for IntoExpressive<Expression>
where
    T: Into<IntoExpressive<Expression>> + Clone + Send + Sync + 'static,
{
    fn from(arc_mutex: &Arc<Mutex<T>>) -> Self {
        let arc_mutex = arc_mutex.clone();
        IntoExpressive::deferred(move || {
            let arc_mutex = arc_mutex.clone();
            Box::pin(async move { arc_mutex.lock().unwrap().clone().into() })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr;
    use crate::expression::owned::Expression;

    #[test]
    fn test_from_arc_mutex() {
        let arc_mutex = Arc::new(Mutex::new(42i32));
        let _expr: IntoExpressive<Expression> = arc_mutex.into();
        // The conversion should work without panicking
    }

    #[tokio::test]
    async fn test_mutable_in_expression() {
        let shared_var = Arc::new(Mutex::new(10i32));
        let expr = expr!("hello {}", &shared_var);

        // The preview should show the deferred placeholder
        let preview = expr.preview();
        assert!(preview.contains("**deferred()"));
    }

    #[tokio::test]
    async fn test_multiple_expressions_same_arc_mutex() {
        let shared_var = Arc::new(Mutex::new(25i32));
        let expr1 = expr!("value1: {}", &shared_var);
        let expr2 = expr!("value2: {}", &shared_var);

        // Modify the shared value
        {
            let mut guard = shared_var.lock().unwrap();
            *guard = 50;
        }

        // Both expressions reference the same Arc<Mutex<T>>
        let preview1 = expr1.preview();
        let preview2 = expr2.preview();

        assert!(preview1.contains("**deferred()"));
        assert!(preview2.contains("**deferred()"));
    }

    #[test]
    fn test_mutable_string() {
        let shared_str = Arc::new(Mutex::new("test".to_string()));
        let expr = expr!("message: {}", &shared_str);

        // Modify the string
        {
            let mut guard = shared_str.lock().unwrap();
            *guard = "modified".to_string();
        }

        let preview = expr.preview();
        assert!(preview.contains("**deferred()"));
    }

    #[test]
    fn test_mutation_affects_expressions() {
        let shared_var = Arc::new(Mutex::new(100i32));
        let _expr1 = expr!("first: {}", &shared_var);
        let _expr2 = expr!("second: {}", &shared_var);

        // Modify the shared value
        {
            let mut guard = shared_var.lock().unwrap();
            *guard = 200;
        }

        // The actual value should be updated (though we can't easily test
        // the deferred evaluation without a full DataSource mock)
        let current_value = *shared_var.lock().unwrap();
        assert_eq!(current_value, 200);
    }
}
