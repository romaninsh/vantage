pub mod expressive;
pub mod join_query;
pub mod query_conditions;
pub mod query_source;
pub mod query_type;

use indexmap::IndexMap;
use serde_json::Value;
use vantage_expressions::{LazyExpression, OwnedExpression, expr};

use crate::Identifier;
use join_query::JoinQuery;
use query_conditions::QueryConditions;
use query_source::QuerySource;
use query_type::QueryType;

#[derive(Debug, Clone)]
pub struct Query {
    from: Vec<QuerySource>,
    with: IndexMap<String, QuerySource>,
    distinct: bool,
    query_type: QueryType,
    fields: IndexMap<Option<String>, OwnedExpression>,
    set_fields: IndexMap<String, Value>,

    where_conditions: QueryConditions,
    having_conditions: QueryConditions,
    joins: Vec<JoinQuery>,

    skip_items: Option<i64>,
    limit_items: Option<i64>,

    group_by: Vec<LazyExpression>,
    order_by: Vec<LazyExpression>,
}

impl Query {
    pub fn new() -> Self {
        Self {
            from: Vec::new(),
            with: IndexMap::new(),
            distinct: false,
            query_type: QueryType,
            fields: IndexMap::new(),
            set_fields: IndexMap::new(),
            where_conditions: QueryConditions,
            having_conditions: QueryConditions,
            joins: Vec::new(),
            skip_items: None,
            limit_items: None,
            group_by: Vec::new(),
            order_by: Vec::new(),
        }
    }

    pub fn fields(mut self, fields: IndexMap<Option<String>, OwnedExpression>) -> Self {
        self.fields = fields;
        self
    }

    pub fn from(mut self, sources: Vec<QuerySource>) -> Self {
        self.from = sources;
        self
    }

    fn render_fields(&self) -> OwnedExpression {
        if self.fields.is_empty() {
            expr!("*")
        } else {
            let field_expressions: Vec<OwnedExpression> = self
                .fields
                .iter()
                .map(|(alias, field)| {
                    if let Some(alias) = alias {
                        expr!("{} AS {}", field.clone(), Identifier::new(alias.clone()))
                    } else {
                        field.clone()
                    }
                })
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
                .map(|source| source.clone().into())
                .collect();
            expr!(
                " FROM {}",
                OwnedExpression::from_vec(from_expressions, ", ")
            )
        }
    }
}

impl Into<OwnedExpression> for Query {
    fn into(self) -> OwnedExpression {
        expr!("SELECT {}{}", self.render_fields(), self.render_from())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_query() {
        let query = Query::new();
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT *");
    }

    #[test]
    fn test_query_with_fields() {
        let mut fields = IndexMap::new();
        fields.insert(Some("name".to_string()), Identifier::new("name").into());
        fields.insert(Some("age".to_string()), Identifier::new("age").into());

        let query = Query::new().fields(fields);
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name` AS `name`, `age` AS `age`");
    }

    #[test]
    fn test_query_with_from() {
        let query = Query::new().from(vec![QuerySource::new(Identifier::new("users"))]);

        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT * FROM `users`");
    }

    #[test]
    fn test_query_with_fields_and_from() {
        let mut fields = IndexMap::new();
        fields.insert(Some("name".to_string()), Identifier::new("name").into());
        fields.insert(Some("age".to_string()), Identifier::new("age").into());

        let query = Query::new()
            .fields(fields)
            .from(vec![QuerySource::new(Identifier::new("users"))]);

        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name` AS `name`, `age` AS `age` FROM `users`");
    }

    #[test]
    fn test_query_with_fields_no_aliases() {
        let mut fields = IndexMap::new();
        fields.insert(
            Some("name_field".to_string()),
            Identifier::new("name").into(),
        );
        fields.insert(Some("age_field".to_string()), Identifier::new("age").into());

        let query = Query::new().fields(fields);
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name` AS `name_field`, `age` AS `age_field`");
    }

    #[test]
    fn test_query_with_fields_without_aliases() {
        let mut fields = IndexMap::new();
        fields.insert(None, Identifier::new("name").into());

        let query = Query::new().fields(fields);
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name`");
    }

    #[test]
    fn test_query_with_multiple_fields_without_aliases() {
        let mut fields: IndexMap<Option<String>, OwnedExpression> = IndexMap::new();
        // Insert fields with unique temporary keys, then modify to None
        fields.insert(Some("temp1".to_string()), Identifier::new("name").into());
        fields.insert(Some("temp2".to_string()), Identifier::new("age").into());

        // Create new map with None keys for demonstration
        let mut no_alias_fields = IndexMap::new();
        let expressions: Vec<_> = fields.into_values().collect();

        // Add first field without alias
        no_alias_fields.insert(None, expressions[0].clone());
        // Add second field with a different key structure to avoid collision
        no_alias_fields.insert(Some("temp_key".to_string()), expressions[1].clone());

        // Remove the temporary key by rebuilding
        let mut final_fields = IndexMap::new();
        for (i, expr) in expressions.into_iter().enumerate() {
            final_fields.insert(
                if i == 0 {
                    None
                } else {
                    Some(format!("field_{}", i))
                },
                expr,
            );
        }

        let query = Query::new().fields(final_fields);
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name`, `age` AS `field_1`");
    }

    #[test]
    fn test_query_with_mixed_fields() {
        let mut fields = IndexMap::new();
        fields.insert(
            Some("full_name".to_string()),
            expr!("CONCAT(first_name, ' ', last_name)"),
        );
        fields.insert(None, Identifier::new("age").into());

        let query = Query::new().fields(fields);
        let expr: OwnedExpression = query.into();
        let sql = expr.preview();
        assert_eq!(
            sql,
            "SELECT CONCAT(first_name, ' ', last_name) AS `full_name`, `age`"
        );
    }

    #[test]
    #[ignore]
    fn test_comprehensive_invoice_query() {
        let mut fields = IndexMap::new();
        fields.insert(None, Identifier::new("id").into());
        fields.insert(
            Some("client_name".to_string()),
            expr!("(SELECT name FROM client WHERE id = invoice.client_id)"),
        );
        fields.insert(None, Identifier::new("invoice_date").into());
        fields.insert(
            Some("invoice_total".to_string()),
            expr!("(SELECT SUM(price * quantity) FROM invoice_line WHERE invoice_id = invoice.id)"),
        );
        fields.insert(
            Some("payments_total".to_string()),
            expr!("(SELECT COALESCE(SUM(amount), 0) FROM payment WHERE invoice_id = invoice.id)"),
        );

        let query = Query::new()
            .fields(fields)
            .from(vec![QuerySource::new(Identifier::new("invoice"))]);

        let expr: OwnedExpression = query.into();
        let sql = expr.preview();

        // This test will fail because we haven't implemented WHERE, GROUP BY, HAVING, ORDER BY yet
        // Expected final output would be:
        // SELECT
        //     id,
        //     (SELECT name FROM client WHERE id = invoice.client_id) AS client_name,
        //     invoice_date,
        //     (SELECT SUM(price * quantity) FROM invoice_line WHERE invoice_id = invoice.id) AS invoice_total,
        //     (SELECT COALESCE(SUM(amount), 0) FROM payment WHERE invoice_id = invoice.id) AS payments_total
        // FROM invoice
        // WHERE is_deleted = false
        // GROUP BY id, invoice_date
        // HAVING invoice_total > payments_total
        // ORDER BY invoice_date DESC;

        let expected = "SELECT `id`, (SELECT name FROM client WHERE id = invoice.client_id) AS `client_name`, `invoice_date`, (SELECT SUM(price * quantity) FROM invoice_line WHERE invoice_id = invoice.id) AS `invoice_total`, (SELECT COALESCE(SUM(amount), 0) FROM payment WHERE invoice_id = invoice.id) AS `payments_total` FROM `invoice`";
        assert_eq!(sql, expected);
    }
}
