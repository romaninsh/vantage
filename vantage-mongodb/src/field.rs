use vantage_expressions::{Expression, expr};

#[derive(Debug, Clone)]
pub enum Field {
    Simple(String),
    Expression {
        expression: Expression,
        alias: Option<String>,
    },
}

impl Field {
    pub fn new_expression(expression: Expression, alias: Option<String>) -> Self {
        Self::Expression { expression, alias }
    }

    pub fn new_simple(field: impl Into<String>) -> Self {
        Self::Simple(field.into())
    }

    pub fn is_expression(&self) -> bool {
        matches!(self, Field::Expression { .. })
    }

    fn needs_quotes(field: &str) -> bool {
        // MongoDB field names with special characters or starting with $ need quotes
        field.starts_with('$')
            || field.contains('.')
            || field.contains(' ')
            || field.contains('-')
            || field.chars().next().map_or(false, |c| c.is_numeric())
    }

    pub fn expression(&self) -> Expression {
        match self {
            Field::Simple(field) => {
                if Self::needs_quotes(field) {
                    expr!(format!("\"{}\"", field))
                } else {
                    expr!(field.clone())
                }
            }
            Field::Expression { expression, .. } => expression.clone(),
        }
    }

    pub fn alias(&self) -> Option<&String> {
        match self {
            Field::Simple(_) => None,
            Field::Expression { alias, .. } => alias.as_ref(),
        }
    }
}

impl Into<Expression> for Field {
    fn into(self) -> Expression {
        self.expression()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_no_quotes() {
        let field = Field::new_simple("username");
        let expr: Expression = field.into();
        assert_eq!(expr.preview(), "username");
    }

    #[test]
    fn test_field_with_dot() {
        let field = Field::new_simple("user.name");
        let expr: Expression = field.into();
        assert_eq!(expr.preview(), "\"user.name\"");
    }

    #[test]
    fn test_field_with_dollar() {
        let field = Field::new_simple("$set");
        let expr: Expression = field.into();
        assert_eq!(expr.preview(), "\"$set\"");
    }

    #[test]
    fn test_field_with_space() {
        let field = Field::new_simple("user name");
        let expr: Expression = field.into();
        assert_eq!(expr.preview(), "\"user name\"");
    }

    #[test]
    fn test_field_starts_with_number() {
        let field = Field::new_simple("1user");
        let expr: Expression = field.into();
        assert_eq!(expr.preview(), "\"1user\"");
    }

    #[test]
    fn test_field_with_expression() {
        let field = Field::new_expression(expr!("quantity*price"), Some("total".to_string()));
        assert_eq!(field.expression().preview(), "quantity*price");
        assert_eq!(field.alias(), Some(&"total".to_string()));
    }

    #[test]
    fn test_field_with_alias() {
        let field = Field::new_expression(expr!("name"), Some("username".to_string()));
        assert_eq!(field.expression().preview(), "name");
        assert_eq!(field.alias(), Some(&"username".to_string()));
    }

    #[test]
    fn test_is_expression() {
        let simple_field = Field::new_simple("name");
        let expr_field = Field::new_expression(expr!("quantity*price"), Some("total".to_string()));

        assert!(!simple_field.is_expression());
        assert!(expr_field.is_expression());
    }
}
