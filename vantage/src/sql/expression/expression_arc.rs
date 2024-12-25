use super::Expression;
use std::sync::Arc;

use crate::{
    sql::chunk::Chunk,
    // operations::Operations,
    traits::column::SqlField,
};

pub trait WrapArc {
    fn wrap_arc(self) -> Arc<Box<dyn Chunk>>;
}
impl<T: Chunk + 'static> WrapArc for T {
    fn wrap_arc(self) -> Arc<Box<dyn Chunk>> {
        Arc::new(Box::new(self))
    }
}
impl WrapArc for Arc<Box<dyn Chunk>> {
    fn wrap_arc(self) -> Arc<Box<dyn Chunk>> {
        self
    }
}

#[macro_export]
macro_rules! expr_arc {
    ($fmt:expr $(, $arg:expr)*) => {{
        ExpressionArc::new(
            $fmt.to_string(),
            vec![
                $( $crate::sql::WrapArc::wrap_arc($arg), )*
            ]
        )
    }}
}

#[derive(Debug)]
pub struct ExpressionArc {
    expression: String,
    parameters: Vec<Arc<Box<dyn Chunk>>>,
}

impl ExpressionArc {
    pub fn new(expression: String, parameters: Vec<Arc<Box<dyn Chunk>>>) -> ExpressionArc {
        ExpressionArc {
            expression,
            parameters,
        }
    }

    pub fn from_vec(vec: Vec<Arc<Box<dyn Chunk>>>, delimiter: &str) -> Self {
        let expression = vec
            .iter()
            .map(|_| "{}")
            .collect::<Vec<&str>>()
            .join(delimiter);

        Self {
            expression,
            parameters: vec,
        }
    }

    pub fn fx(function_name: &str, parameters: Vec<Expression>) -> Self {
        let parameters = Expression::from_vec(parameters, ", ");
        expr_arc!(format!("{}({{}})", function_name), parameters)
    }

    /// Places values into the template and returns a String.
    /// Useful for debugging, but not for SQL execution.
    pub fn preview(&self) -> String {
        self.render_chunk().preview()
    }
}

impl Chunk for ExpressionArc {
    fn render_chunk(&self) -> Expression {
        let token = "{}";

        let mut param_iter = self.parameters.iter();
        let mut sql = self.expression.split(token);

        let mut param_out = Vec::new();
        let mut sql_out: String = String::from(sql.next().unwrap());

        while let Some(param) = param_iter.next() {
            let (param_sql, param_values) = param.render_chunk().split();
            sql_out.push_str(&param_sql);
            param_out.extend(param_values);
            sql_out.push_str(sql.next().unwrap());
        }

        Expression::new(sql_out, param_out)
    }
}

impl SqlField for ExpressionArc {
    fn render_column(&self, alias: Option<&str>) -> Expression {
        let expression = if let Some(alias) = alias {
            format!("({}) AS {}", self.expression, alias)
        } else {
            format!("({})", self.expression)
        };

        ExpressionArc::new(expression, self.parameters.clone()).render_chunk()
    }
    fn calculated(&self) -> bool {
        true
    }
}

// impl Operations for Expression {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use Chunk;

    #[test]
    fn test_expression() {
        let expression = ExpressionArc::new("Hello World".to_string(), vec![]);
        let (sql, params) = expression.render_chunk().split();

        assert_eq!(sql, "Hello World");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_nested() {
        let nested = ExpressionArc::new("Nested".to_string(), vec![]);
        let expression = ExpressionArc::new(
            "Hello {} World".to_string(),
            vec![Arc::new(Box::new(nested))],
        );
        let (sql, params) = expression.render_chunk().split();

        assert_eq!(sql, "Hello Nested World");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_expr_without_parameters() {
        let expression = expr_arc!("Hello World");
        let (sql, params) = expression.render_chunk().split();

        assert_eq!(sql, "Hello World");
        assert_eq!(params.len(), 0);

        let expression = expr_arc!("Hello World".to_string());
        let (sql, params) = expression.render_chunk().split();

        assert_eq!(sql, "Hello World");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_nested_expr_without_parameters() {
        let nested = expr_arc!("Nested");
        let expression = expr_arc!("Hello {} World", nested);
        let (sql, params) = expression.render_chunk().split();

        assert_eq!(sql, "Hello Nested World");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_two_deep_rendering() {
        let expr1 = expr_arc!("{} World", "nested");
        let expr2 = expr_arc!("Hello {}", expr1);

        let (sql, params) = expr2.render_chunk().split();

        assert_eq!(sql, "Hello {} World");
        assert_eq!(params.len(), 1);
        assert_eq!(params, vec![json!("nested")]);
    }

    #[test]
    fn test_nested_expression() {
        let nested = Expression::new("Nested".to_string(), vec![]);
        let expression = expr_arc!("Hello {} World".to_string(), nested);

        let (sql, params) = expression.render_chunk().split();

        assert_eq!(sql, "Hello Nested World");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_multiple_replacements() {
        let a = Arc::new(Box::new(json!(10)) as Box<dyn Chunk>);
        let b = Arc::new(Box::new(json!(5)) as Box<dyn Chunk>);
        let c = Arc::new(Box::new(json!(5)) as Box<dyn Chunk>);
        let expression = ExpressionArc::new("{} - {} = {}".to_string(), vec![a, b, c]);

        let (sql, params) = expression.render_chunk().split();

        assert_eq!(sql, "{} - {} = {}");
        assert_eq!(params.len(), 3);
        assert_eq!(params, vec![json!(10), json!(5), json!(5)]);
    }

    #[test]
    fn test_nested_expr() {
        let a = "10".to_owned();
        let b = "5";
        let c = Arc::new(Box::new(4) as Box<dyn Chunk>); // not double-wrapped

        let expr2 = expr_arc!("{} + {}", b, c);
        let expr1 = expr_arc!("{} + {}", a, expr2);

        let (sql, params) = expr1.render_chunk().split();

        assert_eq!(sql, "{} + {} + {}");
        assert_eq!(params.len(), 3);
        assert_eq!(params, vec![json!("10"), json!("5"), json!(4)]);
    }

    #[test]
    fn test_column() {
        let a = "10".to_owned();
        let b = "5";
        let c = 4;

        let expr2 = expr_arc!("{} + {}", b, c);
        let expr1 = expr_arc!("{} + {}", a, expr2);

        let column = expr1.render_column(Some("result"));
        let (sql, params) = column.split();

        assert_eq!(sql, "({} + {} + {}) AS result");
        assert_eq!(params.len(), 3);
        assert_eq!(params, vec![json!("10"), json!("5"), json!(4)]);
    }

    #[test]
    fn test_lifetimes() {
        let expr2 =
            Arc::new(Box::new(Expression::new("Hello".to_string(), vec![])) as Box<dyn Chunk>);
        {
            let expr1 = ExpressionArc::new("{}".to_string(), vec![expr2.clone()]);
            drop(expr1);
        }

        // we still own expr2
        let _ = expr2;
    }

    #[test]
    fn vec_of_expr() {
        let expr2 = WrapArc::wrap_arc(expr_arc!("name = {}", "John"));
        let expr1 = WrapArc::wrap_arc(expr_arc!("age > {}", 18));

        let vec = vec![expr1, expr2];
        let conditions = ExpressionArc::from_vec(vec, " AND ");

        let expr = expr_arc!("WHERE {}", conditions);

        let (sql, params) = expr.render_chunk().split();

        assert_eq!(sql, "WHERE age > {} AND name = {}");
        assert_eq!(params.len(), 2);
        assert_eq!(params, vec![json!(18), json!("John")]);
    }
}
