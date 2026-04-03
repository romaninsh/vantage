use crate::identifier::Identifier;
use crate::{AnySurrealType, Expr};
use vantage_expressions::ExpressiveOr;
use vantage_expressions::result::QueryResult;

use super::SurrealSelect;
use super::select_field::SelectField;
use super::select_target::SelectTarget;
use vantage_core::IntoVec;
use vantage_expressions::Expressive;

impl SurrealSelect {
    /// Creates a new SELECT query builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the fields for the SELECT clause
    pub fn fields(mut self, fields: Vec<SelectField>) -> Self {
        self.fields = fields;
        self
    }
}

impl<T: QueryResult> SurrealSelect<T> {
    pub fn without_fields(mut self) -> Self {
        self.fields = vec![];
        self
    }

    pub fn with_value(mut self) -> Self {
        self.single_value = true;
        self
    }

    pub fn with_expression(mut self, expression: Expr, alias: Option<String>) -> Self {
        let mut field = SelectField::new(expression);
        if let Some(alias) = alias {
            field = field.with_alias(alias);
        }
        self.fields.push(field);
        self
    }

    pub fn field(mut self, field: impl ExpressiveOr<AnySurrealType, Identifier>) -> Self {
        self.fields.push(SelectField::new(field.field_expr()));
        self
    }

    pub fn with_where(mut self, condition: impl Expressive<AnySurrealType>) -> Self {
        self.where_conditions.push(condition.expr());
        self
    }

    pub fn with_order_by(
        mut self,
        field: impl ExpressiveOr<AnySurrealType, Identifier>,
        ascending: bool,
    ) -> Self {
        self.order_by.push((field.field_expr(), ascending));
        self
    }

    pub fn with_group_by(mut self, field: impl ExpressiveOr<AnySurrealType, Identifier>) -> Self {
        self.group_by.push(field.field_expr());
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_skip(mut self, skip: i64) -> Self {
        self.skip = Some(skip);
        self
    }

    pub fn with_distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    /// Adds targets to the FROM clause
    pub fn add_from(&mut self, targets: impl IntoVec<SelectTarget>) {
        self.from.extend(targets.into_vec());
    }

    /// Builder-style FROM clause
    pub fn from(mut self, targets: impl IntoVec<SelectTarget>) -> Self {
        self.add_from(targets);
        self
    }
}
