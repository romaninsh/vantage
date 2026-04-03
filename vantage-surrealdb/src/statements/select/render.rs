use crate::{AnySurrealType, Expr, surreal_expr};
use vantage_expressions::result::QueryResult;
use vantage_expressions::{Expression, Expressive};

use super::SurrealSelect;
use super::select_target::Target;

impl<T: QueryResult> SurrealSelect<T> {
    /// Renders the SELECT fields clause
    fn render_fields(&self) -> Expr {
        if self.fields.is_empty() {
            surreal_expr!("*")
        } else {
            let field_expressions: Vec<Expr> = self
                .fields
                .iter()
                .map(|field| field.clone().into())
                .collect();
            Expression::from_vec(field_expressions, ", ")
        }
    }

    /// Renders the FROM clause
    fn render_from(&self) -> Expr {
        if self.from.is_empty() {
            surreal_expr!("")
        } else {
            let from_expressions: Vec<Expr> = self
                .from
                .iter()
                .map(|target: &Target| target.clone().into())
                .collect();
            surreal_expr!(
                format!(" FROM {}{{}}", if self.from_only { "ONLY " } else { "" }),
                (Expression::from_vec(from_expressions, ", "))
            )
        }
    }

    /// Renders the WHERE clause
    fn render_where(&self) -> Expr {
        if self.where_conditions.is_empty() {
            surreal_expr!("")
        } else {
            let combined = Expression::from_vec(self.where_conditions.clone(), " AND ");
            surreal_expr!(" WHERE {}", (combined))
        }
    }

    /// Renders the GROUP BY clause
    fn render_group_by(&self) -> Expr {
        if self.group_by.is_empty() {
            surreal_expr!("")
        } else {
            let group_expressions: Vec<Expr> = self.group_by.to_vec();
            surreal_expr!(
                " GROUP BY {}",
                (Expression::from_vec(group_expressions, ", "))
            )
        }
    }

    /// Renders the ORDER BY clause
    fn render_order_by(&self) -> Expr {
        if self.order_by.is_empty() {
            surreal_expr!("")
        } else {
            let order_expressions: Vec<Expr> = self
                .order_by
                .iter()
                .map(|(expression, ascending)| {
                    if *ascending {
                        surreal_expr!("{}", (expression.clone()))
                    } else {
                        surreal_expr!("{} DESC", (expression.clone()))
                    }
                })
                .collect();
            let combined = Expression::from_vec(order_expressions, ", ");
            surreal_expr!(" ORDER BY {}", (combined))
        }
    }

    /// Renders the LIMIT and START clauses
    fn render_limit(&self) -> Expr {
        match (self.limit, self.skip) {
            (Some(limit), Some(skip)) => surreal_expr!(" LIMIT {} START {}", limit, skip),
            (Some(limit), None) => surreal_expr!(" LIMIT {}", limit),
            (None, Some(skip)) => surreal_expr!(" START {}", skip),
            (None, None) => surreal_expr!(""),
        }
    }

    /// Renders entire statement into an expression
    pub(crate) fn render(&self) -> Expr {
        surreal_expr!(
            "SELECT {}{}{}{}{}{}{}",
            (if self.single_value {
                surreal_expr!("VALUE ")
            } else {
                surreal_expr!("")
            }),
            (self.render_fields()),
            (self.render_from()),
            (self.render_where()),
            (self.render_group_by()),
            (self.render_order_by()),
            (self.render_limit())
        )
    }

    /// Renders everything into a string. Use for
    /// debug only. Never or use as part of another query!!
    pub fn preview(&self) -> String {
        self.render().preview()
    }
}

impl<T: QueryResult> Expressive<AnySurrealType> for SurrealSelect<T> {
    fn expr(&self) -> Expr {
        self.render()
    }
}
