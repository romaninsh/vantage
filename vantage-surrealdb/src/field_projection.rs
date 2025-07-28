//! # SurrealDB Field Projection
//!
//! Provides field projection functionality for SurrealDB queries, allowing
//! construction of object projections like `{field: value, alias: expression}`.

use vantage_expressions::{OwnedExpression, expr};

use crate::{identifier::Identifier, operation::Expressive};

/// Represents a field in a field projection
///
/// Used within FieldProjection to represent individual field mappings
/// like `alias: expression`.
#[derive(Debug, Clone)]
pub struct FieldProjectionField {
    alias: String,
    expression: OwnedExpression,
}

impl FieldProjectionField {
    /// Creates a new field projection field
    pub fn new(alias: impl Into<String>, expression: impl Into<OwnedExpression>) -> Self {
        Self {
            alias: alias.into(),
            expression: expression.into(),
        }
    }
}

impl Expressive for FieldProjectionField {
    fn expr(&self) -> OwnedExpression {
        expr!(
            "{}: {}",
            Identifier::new(self.alias.clone()),
            self.expression.expr()
        )
    }
}

impl Into<OwnedExpression> for FieldProjectionField {
    fn into(self) -> OwnedExpression {
        self.expr()
    }
}

/// Field projection builder for SurrealDB object construction
///
/// Builds field projections in the format `{field1: value1, field2: value2}`.
/// Used for transforming query results into structured objects.
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::field_projection::FieldProjection;
/// use vantage_expressions::expr;
///
/// let projection = FieldProjection::new(expr!("lines[*]"))
///     .with_field("quantity")
///     .with_expression(expr!("product.name"), "product_name")
///     .with_expression(expr!("quantity * price"), "subtotal");
/// ```
#[derive(Debug, Clone)]
pub struct FieldProjection {
    base: Option<OwnedExpression>,
    fields: Vec<FieldProjectionField>,
}

impl FieldProjection {
    /// Creates a new field projection with a base expression
    pub fn new(base: impl Into<OwnedExpression>) -> Self {
        Self {
            base: Some(base.into()),
            fields: Vec::new(),
        }
    }

    /// Adds a field that maps to itself (field_name: field_name)
    ///
    /// # Arguments
    ///
    /// * `field_name` - The field name that will be both the key and value
    pub fn with_field(mut self, field_name: impl Into<String>) -> Self {
        self.add_field(field_name);
        self
    }

    /// Adds a field with an expression (alternative method signature)
    ///
    /// # Arguments
    ///
    /// * `expression` - The expression to evaluate for this field
    /// * `alias` - The field name/alias in the resulting object
    pub fn with_expression(
        mut self,
        expression: impl Into<OwnedExpression>,
        alias: impl Into<String>,
    ) -> Self {
        self.add_expression(expression, alias);
        self
    }

    /// Adds a field that maps to itself (mutable version)
    ///
    /// # Arguments
    ///
    /// * `field_name` - The field name that will be both the key and value
    pub fn add_field(&mut self, field_name: impl Into<String>) {
        let field_name = field_name.into();
        self.fields.push(FieldProjectionField::new(
            field_name.clone(),
            expr!(field_name),
        ));
    }

    /// Adds a field with an expression (mutable version, alternative signature)
    ///
    /// # Arguments
    ///
    /// * `expression` - The expression to evaluate for this field
    /// * `alias` - The field name/alias in the resulting object
    pub fn add_expression(
        &mut self,
        expression: impl Into<OwnedExpression>,
        alias: impl Into<String>,
    ) {
        self.fields
            .push(FieldProjectionField::new(alias, expression));
    }
}

impl Expressive for FieldProjection {
    fn expr(&self) -> OwnedExpression {
        let field_expressions =
            OwnedExpression::from_vec(self.fields.iter().map(|f| f.expr()).collect(), ", ");
        let base = self.base.clone().unwrap();

        if base.preview().is_empty() {
            OwnedExpression::new("{{}}", vec![field_expressions.into()])
        } else {
            OwnedExpression::new("{}.{{}}", vec![base.into(), field_expressions.into()])
        }
    }
}

impl Into<OwnedExpression> for FieldProjection {
    fn into(self) -> OwnedExpression {
        self.expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_expressions::expr;

    #[test]
    fn test_empty_projection() {
        let projection = FieldProjection::new(expr!("lines[*]"));
        assert_eq!(projection.expr().preview(), "lines[*].{}");
    }

    #[test]
    fn test_single_field() {
        let projection = FieldProjection::new(expr!("lines[*]")).with_field("quantity");

        assert_eq!(projection.expr().preview(), "lines[*].{quantity: quantity}");
    }

    #[test]
    fn test_multiple_fields() {
        let projection = FieldProjection::new(expr!("lines[*]"))
            .with_field("quantity")
            .with_field("price")
            .with_expression(expr!("product.name"), "product_name")
            .with_expression(expr!("quantity * price"), "subtotal");

        let expected = "lines[*].{quantity: quantity, price: price, product_name: product.name, subtotal: quantity * price}";

        assert_eq!(projection.expr().preview(), expected);
    }

    #[test]
    fn test_mutable_methods() {
        let mut projection = FieldProjection::new(expr!("items[*]"));
        projection.add_field("name");
        projection.add_expression(expr!("count(*)"), "total");

        let expected = "items[*].{name: name, total: count(*)}";

        assert_eq!(projection.expr().preview(), expected);
    }

    #[test]
    fn test_example_from_query07() {
        let projection = FieldProjection::new(expr!("lines[*]"))
            .with_field("quantity")
            .with_field("price")
            .with_expression(expr!("product.name"), "product_name")
            .with_expression(expr!("quantity * price"), "subtotal");

        let expected = "lines[*].{quantity: quantity, price: price, product_name: product.name, subtotal: quantity * price}";
        assert_eq!(projection.expr().preview(), expected);
    }

    #[test]
    fn test_empty_base() {
        let projection = FieldProjection::new(expr!(""))
            .with_field("quantity")
            .with_field("price")
            .with_expression(expr!("product.name"), "product_name");

        let expected = "{quantity: quantity, price: price, product_name: product.name}";
        assert_eq!(projection.expr().preview(), expected);
    }

    #[test]
    fn test_empty_base_empty_fields() {
        let projection = FieldProjection::new(expr!(""));
        assert_eq!(projection.expr().preview(), "{}");
    }
}
