use vantage_expressions::{Expression, Expressive, ExpressiveEnum, Order};

use crate::mysql::types::AnyMysqlType;

/// Helper: create an inline SQL string literal (single-quoted, with escaping).
fn sql_lit(s: &str) -> Expr {
    let escaped = s.replace('\'', "''");
    Expression::new(format!("'{escaped}'"), vec![])
}

type Expr = Expression<AnyMysqlType>;

/// MySQL GROUP_CONCAT aggregate function.
///
/// Builds `GROUP_CONCAT([DISTINCT] expr [ORDER BY expr [ASC|DESC]] [SEPARATOR 'sep'])`.
///
/// # Examples
///
/// ```ignore
/// // GROUP_CONCAT(DISTINCT p.name ORDER BY p.name SEPARATOR ', ')
/// GroupConcat::new(ident("name").dot_of("p"))
///     .distinct()
///     .order_by(ident("name").dot_of("p"), Order::Asc)
///     .separator(", ")
/// ```
#[derive(Debug, Clone)]
pub struct GroupConcat {
    expr: Expr,
    distinct: bool,
    order_by: Vec<(Expr, Order)>,
    separator: Option<String>,
}

impl GroupConcat {
    pub fn new(expr: impl Expressive<AnyMysqlType>) -> Self {
        Self {
            expr: expr.expr(),
            distinct: false,
            order_by: Vec::new(),
            separator: None,
        }
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn order_by(mut self, expr: impl Expressive<AnyMysqlType>, order: Order) -> Self {
        self.order_by.push((expr.expr(), order));
        self
    }

    pub fn separator(mut self, sep: impl Into<String>) -> Self {
        self.separator = Some(sep.into());
        self
    }

}

impl Expressive<AnyMysqlType> for GroupConcat {
    fn expr(&self) -> Expr {
        // Build the inner parts: [DISTINCT] expr [ORDER BY ...] [SEPARATOR '...']
        let mut parts: Vec<Expr> = Vec::new();

        if self.distinct {
            parts.push(Expression::new(
                "DISTINCT {}",
                vec![ExpressiveEnum::Nested(self.expr.clone())],
            ));
        } else {
            parts.push(self.expr.clone());
        }

        if !self.order_by.is_empty() {
            let order_parts: Vec<Expr> = self
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

        if let Some(sep) = &self.separator {
            parts.push(Expression::new(
                "SEPARATOR {}",
                vec![ExpressiveEnum::Nested(sql_lit(sep))],
            ));
        }

        let inner = Expression::from_vec(parts, " ");
        let base = Expression::new("GROUP_CONCAT({})", vec![ExpressiveEnum::Nested(inner)]);

        base
    }
}
