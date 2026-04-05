use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// SQL CASE expression: `CASE WHEN cond THEN val ... ELSE val END`.
///
/// # Examples
///
/// ```ignore
/// Case::new()
///     .when(sqlite_expr!("{} >= {}", (Identifier::new("salary")), 100000.0f64), sqlite_expr!("{}", "senior"))
///     .when(sqlite_expr!("{} >= {}", (Identifier::new("salary")), 60000.0f64), sqlite_expr!("{}", "mid"))
///     .else_(sqlite_expr!("{}", "intern"))
///     .with_alias("band")
/// ```
#[derive(Debug, Clone)]
pub struct Case<T: Debug + Display + Clone> {
    branches: Vec<(Expression<T>, Expression<T>)>,
    else_branch: Option<Expression<T>>,
    alias: Option<String>,
}

impl<T: Debug + Display + Clone> Case<T> {
    pub fn new() -> Self {
        Self {
            branches: Vec::new(),
            else_branch: None,
            alias: None,
        }
    }

    pub fn when(mut self, condition: impl Expressive<T>, then: impl Expressive<T>) -> Self {
        self.branches.push((condition.expr(), then.expr()));
        self
    }

    pub fn else_(mut self, value: impl Expressive<T>) -> Self {
        self.else_branch = Some(value.expr());
        self
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

impl<T: Debug + Display + Clone> Expressive<T> for Case<T> {
    fn expr(&self) -> Expression<T> {
        let mut parts: Vec<Expression<T>> = Vec::new();
        for (cond, then) in &self.branches {
            parts.push(Expression::new(
                " WHEN {} THEN {}",
                vec![
                    ExpressiveEnum::Nested(cond.clone()),
                    ExpressiveEnum::Nested(then.clone()),
                ],
            ));
        }

        let branches_expr = Expression::from_vec(parts, "");

        let base = match &self.else_branch {
            Some(else_val) => Expression::new(
                "CASE{} ELSE {} END",
                vec![
                    ExpressiveEnum::Nested(branches_expr),
                    ExpressiveEnum::Nested(else_val.clone()),
                ],
            ),
            None => Expression::new(
                "CASE{} END",
                vec![ExpressiveEnum::Nested(branches_expr)],
            ),
        };

        match &self.alias {
            Some(alias) => Expression::new(
                format!("{{}} AS \"{}\"", alias),
                vec![ExpressiveEnum::Nested(base)],
            ),
            None => base,
        }
    }
}
