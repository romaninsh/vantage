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
        if self.conditions.is_empty() {
            return crate::surreal_expr!("DELETE {}", (self.target));
        }

        let combined = self
            .conditions
            .iter()
            .cloned()
            .reduce(|a, b| crate::surreal_expr!("{} AND {}", (a), (b)))
            .unwrap();

        crate::surreal_expr!("DELETE {} WHERE {}", (self.target), (combined))
    }
}

impl From<SurrealDelete> for Expr {
    fn from(delete: SurrealDelete) -> Self {
        delete.expr()
    }
}
