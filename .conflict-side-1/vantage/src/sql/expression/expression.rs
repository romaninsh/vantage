use serde_json::Value;

use crate::{sql::Operations, sql::chunk::Chunk, traits::column::SqlField};

/// Constructs [`Expression`] from a format scring and several parameters by passing those
/// into [`json!`]
///
/// ```
/// let my_sum = expr!("{} + {}", 2, 3);
/// ```
///
/// The parameter to the expr! can be anything that you can also pass into [`json!`] macro
///
/// [`json!`]: serde_json::json!

#[macro_export]
macro_rules! expr {
    ($fmt:expr $(, $arg:expr)*) => {{
        Expression::new(
            $fmt.to_string(),
            vec![
                $( serde_json::json!($arg), )*
            ]
        )
    }}
}

/// Expression is a basic piece of SQL query template that contains a format string
/// and several parameters. Easiest way to create Expression is with [`expr!`] macro
///

#[derive(Debug, Clone)]
pub struct Expression {
    expression: String,
    parameters: Vec<Value>,
}

/// Expression can be used anywhere, where SqlChunk is accepted. For example:
/// ```
/// let expression = expr_arc!("{} + ({})", 2, expr!("3 * 4"));
/// ```
impl Chunk for Expression {
    fn render_chunk(&self) -> Expression {
        self.clone()
    }
}

impl Expression {
    pub fn new(expression: String, parameters: Vec<Value>) -> Self {
        Self {
            expression,
            parameters,
        }
    }

    pub fn as_type(value: Value, as_type: &str) -> Self {
        expr!(format!("{{}}::{}", as_type), value)
    }

    pub fn empty() -> Self {
        Self {
            expression: "".to_owned(),
            parameters: vec![],
        }
    }

    /// Return "SQL" template part of the expression
    pub fn sql(&self) -> &String {
        &self.expression
    }

    /// Converts template by replacing corresponding {} placeholders with $1, $2 etc,
    /// which is more suitable for some SQL crates
    ///
    /// ```
    /// let final = expr!("{} + {}", 2, 3);  // "$1 + $2"
    /// ```
    pub fn sql_final(&self) -> String {
        let mut sql_final = self.expression.clone();

        let token = "{}";
        let mut num = 0;
        while let Some(index) = sql_final.find(token) {
            num += 1;
            sql_final.replace_range(index..index + token.len(), &format!("${}", num));
        }
        sql_final
    }

    pub fn params(&self) -> &Vec<Value> {
        &self.parameters
    }

    /// Given a `Vec<Expression>` and a delimeter, will construct a new expression,
    /// by combining all nested templates together:
    /// ```
    /// let e1 = expr!("hello {}", "world");
    /// let e2 = expr!("foo {}", "bar");
    /// let e = Expression::from_vec(vec![e1, e2], " <=> ");
    ///
    /// writeln(e.sql()); // hello {} <=> foo {}
    /// ```
    pub fn from_vec(vec: Vec<Expression>, delimiter: &str) -> Self {
        let expression = vec
            .iter()
            .map(|pre| pre.expression.clone())
            .collect::<Vec<String>>()
            .join(delimiter);

        let parameters = vec
            .into_iter()
            .map(|pre| pre.parameters)
            .flatten()
            .collect::<Vec<Value>>();

        Self {
            expression,
            parameters,
        }
    }

    /// Return SQL template and parameter vec as a tuple
    pub fn split(self) -> (String, Vec<Value>) {
        (self.expression, self.parameters)
    }

    /// Places values into the template and returns a String.
    /// Useful for debugging, but not for SQL execution.
    pub fn preview(&self) -> String {
        let mut preview = self.expression.clone();
        for param in &self.parameters {
            preview = preview.replacen("{}", &param.to_string(), 1);
        }
        preview
    }
}

impl SqlField for Expression {
    fn render_column(&self, alias: Option<&str>) -> Expression {
        let expression = if let Some(alias) = alias {
            format!("({}) AS {}", self.expression, alias)
        } else {
            format!("({})", self.expression)
        };

        Expression::new(expression, self.parameters.clone())
    }
    fn calculated(&self) -> bool {
        true
    }
}
impl Operations for Expression {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::chunk::Chunk;
    use serde_json::json;

    #[test]
    fn test_as_type() {
        let expression = Expression::as_type(json!(1), "int");
        let (sql, params) = expression.render_chunk().split();
        assert_eq!(sql, "{}::int");
        assert_eq!(params, vec![Value::Number(1.into())]);
    }

    #[test]
    fn test_expr_macro() {
        let expr = expr!("{} + {}", 2, 2);
        assert_eq!(expr.preview(), "2 + 2");
    }
}
