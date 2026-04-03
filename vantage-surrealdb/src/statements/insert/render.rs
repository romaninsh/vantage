use crate::Expr;
use crate::identifier::Identifier;
use crate::types::AnySurrealType;
use vantage_expressions::Expressive;

use super::SurrealInsert;

impl SurrealInsert {
    /// Render the statement as a string (for debugging — never use in queries).
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnySurrealType> for SurrealInsert {
    fn expr(&self) -> Expr {
        let target = self.target_expr();

        if self.fields.is_empty() {
            return crate::surreal_expr!("CREATE {}", (target));
        }

        // Build "key1 = {}, key2 = {}" with field values as scalar params
        let keys: Vec<&String> = self.fields.keys().collect();
        let placeholders: Vec<String> = keys
            .iter()
            .map(|k| format!("{} = {{}}", Identifier::new(*k).expr().preview()))
            .collect();
        let template = format!("CREATE {{}} SET {}", placeholders.join(", "));

        let mut params: Vec<vantage_expressions::ExpressiveEnum<AnySurrealType>> =
            vec![vantage_expressions::ExpressiveEnum::Nested(target)];

        for value in self.fields.values() {
            params.push(vantage_expressions::ExpressiveEnum::Scalar(value.clone()));
        }

        vantage_expressions::Expression::new(template, params)
    }
}

impl From<SurrealInsert> for Expr {
    fn from(insert: SurrealInsert) -> Self {
        insert.expr()
    }
}
