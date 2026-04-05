use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::sqlite::types::AnySqliteType;

use super::{Expr, SqliteSelect};

impl SqliteSelect {
    fn render_fields(&self) -> Expr {
        if self.fields.is_empty() {
            Expression::new("*", vec![])
        } else {
            Expression::from_vec(self.fields.clone(), ", ")
        }
    }

    fn render_from(&self) -> Expr {
        if self.from.is_empty() {
            Expression::new("", vec![])
        } else {
            Expression::new(
                " FROM {}",
                vec![ExpressiveEnum::Nested(Expression::from_vec(
                    self.from.clone(),
                    ", ",
                ))],
            )
        }
    }

    fn render_where(&self) -> Expr {
        if self.where_conditions.is_empty() {
            Expression::new("", vec![])
        } else {
            let combined = Expression::from_vec(self.where_conditions.clone(), " AND ");
            Expression::new(" WHERE {}", vec![ExpressiveEnum::Nested(combined)])
        }
    }

    fn render_group_by(&self) -> Expr {
        if self.group_by.is_empty() {
            Expression::new("", vec![])
        } else {
            Expression::new(
                " GROUP BY {}",
                vec![ExpressiveEnum::Nested(Expression::from_vec(
                    self.group_by.clone(),
                    ", ",
                ))],
            )
        }
    }

    fn render_order_by(&self) -> Expr {
        if self.order_by.is_empty() {
            Expression::new("", vec![])
        } else {
            let parts: Vec<Expr> = self
                .order_by
                .iter()
                .map(|(expr, asc)| {
                    if *asc {
                        expr.clone()
                    } else {
                        Expression::new(
                            "{} DESC",
                            vec![ExpressiveEnum::Nested(expr.clone())],
                        )
                    }
                })
                .collect();
            Expression::new(
                " ORDER BY {}",
                vec![ExpressiveEnum::Nested(Expression::from_vec(parts, ", "))],
            )
        }
    }

    fn render_limit(&self) -> Expr {
        match (self.limit, self.skip) {
            (Some(limit), Some(skip)) => Expression::new(
                " LIMIT {} OFFSET {}",
                vec![
                    ExpressiveEnum::Scalar(AnySqliteType::new(limit)),
                    ExpressiveEnum::Scalar(AnySqliteType::new(skip)),
                ],
            ),
            (Some(limit), None) => Expression::new(
                " LIMIT {}",
                vec![ExpressiveEnum::Scalar(AnySqliteType::new(limit))],
            ),
            (None, Some(skip)) => Expression::new(
                " OFFSET {}",
                vec![ExpressiveEnum::Scalar(AnySqliteType::new(skip))],
            ),
            (None, None) => Expression::new("", vec![]),
        }
    }

    pub fn render(&self) -> Expr {
        Expression::new(
            format!(
                "SELECT{} {{}}{{}}{{}}{{}}{{}}{{}}",
                if self.distinct { " DISTINCT" } else { "" }
            ),
            vec![
                ExpressiveEnum::Nested(self.render_fields()),
                ExpressiveEnum::Nested(self.render_from()),
                ExpressiveEnum::Nested(self.render_where()),
                ExpressiveEnum::Nested(self.render_group_by()),
                ExpressiveEnum::Nested(self.render_order_by()),
                ExpressiveEnum::Nested(self.render_limit()),
            ],
        )
    }

    pub fn preview(&self) -> String {
        self.render().preview()
    }
}

impl Expressive<AnySqliteType> for SqliteSelect {
    fn expr(&self) -> Expr {
        self.render()
    }
}
