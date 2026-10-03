use crate::Expr;
use crate::types::AnySurrealType;
use vantage_expressions::Expressive;

use super::SurrealDelete;

impl SurrealDelete {
    /// Render the statement as a string (for debugging — never use in queries).
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnySurrealType> for SurrealDelete {
    fn expr(&self) -> Expr {
        let base = match self
            .conditions
            .iter()
            .cloned()
            .reduce(|a, b| crate::surreal_expr!("{} AND {}", (a), (b)))
        {
            None => crate::surreal_expr!("DELETE {}", (self.target)),
            Some(combined) => {
                crate::surreal_expr!("DELETE {} WHERE {}", (self.target), (combined))
            }
        };

        if self.return_before {
            crate::surreal_expr!("{} RETURN BEFORE", (base))
        } else {
            base
        }
    }
}

impl From<SurrealDelete> for Expr {
    fn from(delete: SurrealDelete) -> Self {
        delete.expr()
    }
}
