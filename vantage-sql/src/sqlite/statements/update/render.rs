use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use crate::primitives::identifier::ident;
use crate::sqlite::types::AnySqliteType;

use super::{Expr, SqliteUpdate};

impl SqliteUpdate {
    fn render_where(&self) -> Option<Expr> {
        if self.conditions.is_empty() {
            return None;
        }
        Some(Expression::from_vec(self.conditions.clone(), " AND "))
    }

    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnySqliteType> for SqliteUpdate {
    fn expr(&self) -> Expr {
        if self.fields.is_empty() {
            return expr_any!("UPDATE {}", (ident(&self.table)));
        }

        let set_parts: Vec<Expr> = self
            .fields
            .iter()
            .map(|(k, v)| {
                Expression::new(
                    "{} = {}",
                    vec![
                        ExpressiveEnum::Nested(ident(k).expr()),
                        ExpressiveEnum::Scalar(v.clone()),
                    ],
                )
            })
            .collect();
        let set_list = Expression::from_vec(set_parts, ", ");

        match self.render_where() {
            Some(cond) => expr_any!(
                "UPDATE {} SET {} WHERE {}",
                (ident(&self.table)),
                (set_list),
                (cond)
            ),
            None => expr_any!("UPDATE {} SET {}", (ident(&self.table)), (set_list)),
        }
    }
}

impl From<SqliteUpdate> for Expr {
    fn from(update: SqliteUpdate) -> Self {
        update.expr()
    }
}
