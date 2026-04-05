use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::sqlite::types::AnySqliteType;

use super::{Expr, SqliteSelect};

fn render_condition_list(conditions: &[Expr], keyword: &str) -> Expr {
    if conditions.is_empty() {
        Expression::new("", vec![])
    } else {
        let combined = Expression::from_vec(conditions.to_vec(), " AND ");
        Expression::new(
            format!(" {} {{}}", keyword),
            vec![ExpressiveEnum::Nested(combined)],
        )
    }
}

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

    fn render_joins(&self) -> Expr {
        if self.joins.is_empty() {
            Expression::new("", vec![])
        } else {
            let parts: Vec<Expr> = self.joins.iter().map(|j| j.render()).collect();
            Expression::from_vec(parts, "")
        }
    }

    fn render_where(&self) -> Expr {
        render_condition_list(&self.where_conditions, "WHERE")
    }

    fn render_having(&self) -> Expr {
        render_condition_list(&self.having, "HAVING")
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

    fn render_windows(&self) -> Expr {
        if self.windows.is_empty() {
            Expression::new("", vec![])
        } else {
            let parts: Vec<Expr> = self
                .windows
                .iter()
                .map(|(name, win)| {
                    Expression::new(
                        format!("{} AS {{}}", name),
                        vec![ExpressiveEnum::Nested(win.definition())],
                    )
                })
                .collect();
            Expression::new(
                " WINDOW {}",
                vec![ExpressiveEnum::Nested(Expression::from_vec(parts, ", "))],
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
                        Expression::new("{} DESC", vec![ExpressiveEnum::Nested(expr.clone())])
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

    fn render_ctes(&self) -> Expr {
        if self.ctes.is_empty() {
            Expression::new("", vec![])
        } else {
            let is_recursive = self.ctes.iter().any(|(_, _, r)| *r);
            let parts: Vec<Expr> = self
                .ctes
                .iter()
                .map(|(name, query, _)| {
                    Expression::new(
                        format!("{} AS ({{}})", name),
                        vec![ExpressiveEnum::Nested(query.clone())],
                    )
                })
                .collect();
            let keyword = if is_recursive {
                "WITH RECURSIVE"
            } else {
                "WITH"
            };
            Expression::new(
                format!("{} {{}} ", keyword),
                vec![ExpressiveEnum::Nested(Expression::from_vec(parts, ", "))],
            )
        }
    }

    pub fn render(&self) -> Expr {
        Expression::new(
            format!(
                "{{}}SELECT{} {{}}{{}}{{}}{{}}{{}}{{}}{{}}{{}}{{}}",
                if self.distinct { " DISTINCT" } else { "" }
            ),
            vec![
                ExpressiveEnum::Nested(self.render_ctes()),
                ExpressiveEnum::Nested(self.render_fields()),
                ExpressiveEnum::Nested(self.render_from()),
                ExpressiveEnum::Nested(self.render_joins()),
                ExpressiveEnum::Nested(self.render_where()),
                ExpressiveEnum::Nested(self.render_group_by()),
                ExpressiveEnum::Nested(self.render_having()),
                ExpressiveEnum::Nested(self.render_windows()),
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
