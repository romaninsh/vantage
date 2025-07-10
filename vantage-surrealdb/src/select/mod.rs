// pub mod expressive;
pub mod field;
pub mod select_field;
pub mod target;

use field::Field;
use select_field::SelectField;
use target::Target;

use vantage_expressions::{OwnedExpression, expr};

#[derive(Debug, Clone)]
pub struct Select {
    pub fields: Vec<SelectField>, // SELECT clause fields
    pub fields_omit: Vec<Field>,
    pub from: Vec<Target>, // FROM clause targets
    pub from_omit: bool,
    // pub with: Vec<Index>,

    // pub where_conditions: Option<Expression>,
    // pub split: Vec<Field>,
    // pub group_by: Vec<Field>,
    // pub order_by: Vec<OrderField>,
    // pub limit: Option<u64>,
    // pub start: Option<u64>,
    // pub fetch: Vec<Field>,
    // pub timeout: Option<Duration>,
    // pub version: Option<DateTime>,
}

impl Select {
    pub fn new() -> Self {
        Self {
            fields: Vec::new(),
            fields_omit: Vec::new(),
            from: Vec::new(),
            from_omit: false,
        }
    }

    pub fn fields(mut self, fields: Vec<SelectField>) -> Self {
        self.fields = fields;
        self
    }

    pub fn from(mut self, targets: Vec<Target>) -> Self {
        self.from = targets;
        self
    }

    fn render_fields(&self) -> OwnedExpression {
        if self.fields.is_empty() {
            expr!("*")
        } else {
            let field_expressions: Vec<OwnedExpression> = self
                .fields
                .iter()
                .map(|field| field.clone().into())
                .collect();
            OwnedExpression::from_vec(field_expressions, ", ")
        }
    }

    fn render_from(&self) -> OwnedExpression {
        if self.from.is_empty() {
            expr!("")
        } else {
            let from_expressions: Vec<OwnedExpression> = self
                .from
                .iter()
                .map(|target| target.clone().into())
                .collect();
            expr!(
                " FROM {}",
                OwnedExpression::from_vec(from_expressions, ", ")
            )
        }
    }
}

impl Into<OwnedExpression> for Select {
    fn into(self) -> OwnedExpression {
        expr!("SELECT {}{}", self.render_fields(), self.render_from())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::field::Field;
    use crate::select::select_field::SelectField;
    use crate::select::target::Target;

    #[test]
    fn test_basic_select() {
        let select = Select::new()
            .fields(vec![
                SelectField::new(Field::new("name")),
                SelectField::new(Field::new("set")),
            ])
            .from(vec![Target::new(expr!("users"))]);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT name, ⟨set⟩ FROM users");
    }

    #[test]
    fn test_select_all() {
        let select = Select::new().from(vec![Target::new(expr!("users"))]);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT * FROM users");
    }
}
