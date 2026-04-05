use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// SQL IIF(condition, true_val, false_val) expression.
///
/// # Examples
///
/// ```ignore
/// Iif::new(
///     sqlite_expr!("{} = {}", (Identifier::new("role")), "admin"),
///     sqlite_expr!("{}", "Yes"),
///     sqlite_expr!("{}", "No"),
/// ).with_alias("is_admin")
/// ```
#[derive(Debug, Clone)]
pub struct Iif<T: Debug + Display + Clone> {
    condition: Expression<T>,
    true_val: Expression<T>,
    false_val: Expression<T>,
    alias: Option<String>,
}

impl<T: Debug + Display + Clone> Iif<T> {
    pub fn new(
        condition: impl Expressive<T>,
        true_val: impl Expressive<T>,
        false_val: impl Expressive<T>,
    ) -> Self {
        Self {
            condition: condition.expr(),
            true_val: true_val.expr(),
            false_val: false_val.expr(),
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

impl<T: Debug + Display + Clone> Expressive<T> for Iif<T> {
    fn expr(&self) -> Expression<T> {
        let base = Expression::new(
            "IIF({}, {}, {})",
            vec![
                ExpressiveEnum::Nested(self.condition.clone()),
                ExpressiveEnum::Nested(self.true_val.clone()),
                ExpressiveEnum::Nested(self.false_val.clone()),
            ],
        );

        match &self.alias {
            Some(alias) => Expression::new(
                format!("{{}} AS \"{}\"", alias),
                vec![ExpressiveEnum::Nested(base)],
            ),
            None => base,
        }
    }
}
