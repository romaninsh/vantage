use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// SQL window specification for window functions.
///
/// # Examples
///
/// ```ignore
/// let win = Window::new()
///     .partition_by(ident("department_id").dot_of("u"))
///     .order_by(ident("salary").dot_of("u"), Order::Desc);
///
/// // Inline: SUM(u.salary) OVER (PARTITION BY ... ORDER BY ... DESC)
/// win.apply(Fx::new("sum", [ident("salary").dot_of("u").expr()]))
///
/// // Named reference: ROW_NUMBER() OVER win
/// Window::named("win").apply(Fx::new("row_number", []))
/// ```
#[derive(Debug, Clone)]
pub struct Window<T: Debug + Display + Clone> {
    partition_by: Vec<Expression<T>>,
    order_by: Vec<(Expression<T>, vantage_expressions::Order)>,
    frame: Option<String>,
    named_ref: Option<String>,
}

impl<T: Debug + Display + Clone> Default for Window<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Debug + Display + Clone> Window<T> {
    pub fn new() -> Self {
        Self {
            partition_by: Vec::new(),
            order_by: Vec::new(),
            frame: None,
            named_ref: None,
        }
    }

    /// Reference a named window defined in WINDOW clause.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            partition_by: Vec::new(),
            order_by: Vec::new(),
            frame: None,
            named_ref: Some(name.into()),
        }
    }

    pub fn partition_by(mut self, expr: impl Expressive<T>) -> Self {
        self.partition_by.push(expr.expr());
        self
    }

    pub fn order_by(mut self, expr: impl Expressive<T>, order: vantage_expressions::Order) -> Self {
        self.order_by.push((expr.expr(), order));
        self
    }

    pub fn rows(mut self, from: &str, to: &str) -> Self {
        self.frame = Some(format!("ROWS BETWEEN {} AND {}", from, to));
        self
    }

    pub fn range(mut self, from: &str, to: &str) -> Self {
        self.frame = Some(format!("RANGE BETWEEN {} AND {}", from, to));
        self
    }

    /// Apply a function over this window: `expr OVER (spec)` or `expr OVER name`.
    pub fn apply(&self, func: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} OVER {}",
            vec![
                ExpressiveEnum::Nested(func.expr()),
                ExpressiveEnum::Nested(self.spec_expr()),
            ],
        )
    }

    /// Render the window spec — either a named reference or inline `(...)`.
    fn spec_expr(&self) -> Expression<T> {
        if let Some(name) = &self.named_ref {
            return Expression::new(name.clone(), vec![]);
        }

        let mut parts: Vec<Expression<T>> = Vec::new();

        if !self.partition_by.is_empty() {
            parts.push(Expression::new(
                "PARTITION BY {}",
                vec![ExpressiveEnum::Nested(Expression::from_vec(
                    self.partition_by.clone(),
                    ", ",
                ))],
            ));
        }

        if !self.order_by.is_empty() {
            let order_parts: Vec<Expression<T>> = self
                .order_by
                .iter()
                .map(|(expr, order)| {
                    Expression::new(
                        format!("{{}}{}", order.suffix()),
                        vec![ExpressiveEnum::Nested(expr.clone())],
                    )
                })
                .collect();
            parts.push(Expression::new(
                "ORDER BY {}",
                vec![ExpressiveEnum::Nested(Expression::from_vec(
                    order_parts,
                    ", ",
                ))],
            ));
        }

        if let Some(frame) = &self.frame {
            parts.push(Expression::new(frame.clone(), vec![]));
        }

        let inner = Expression::from_vec(parts, " ");
        Expression::new("({})", vec![ExpressiveEnum::Nested(inner)])
    }

    /// Render just the definition part (for WINDOW clause): `(PARTITION BY ... ORDER BY ...)`
    pub fn definition(&self) -> Expression<T> {
        self.spec_expr()
    }
}
