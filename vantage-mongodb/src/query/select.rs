use crate::field::Field;
use async_trait::async_trait;
use serde_json::Value;
use std::fmt::Debug;
use vantage_expressions::{Expr, Expression, expr, protocol::selectable::Selectable};

#[derive(Debug, Clone)]
pub struct MongoSelect {
    source: Option<Expression>,
    source_alias: Option<String>,
    fields: Vec<Field>,
    where_conditions: Vec<Expression>,
    order_by: Vec<(Expression, bool)>,
    group_by: Vec<Expression>,
    distinct: bool,
    limit: Option<i64>,
    skip: Option<i64>,
}

impl MongoSelect {
    pub fn new() -> Self {
        Self {
            source: None,
            source_alias: None,
            fields: Vec::new(),
            where_conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            distinct: false,
            limit: None,
            skip: None,
        }
    }

    fn has_expressions(&self) -> bool {
        self.fields.iter().any(|field| field.is_expression())
    }

    fn render_find(&self) -> Expression {
        let collection = if let Some(ref source) = self.source {
            source.clone()
        } else {
            expr!("collection")
        };

        let filter = if self.where_conditions.is_empty() {
            expr!("{}")
        } else {
            self.combine_where_conditions()
        };

        // Build the base query using string formatting to ensure proper structure
        let mut base_query = if self.fields.is_empty() {
            format!("db.{}.find({})", collection.preview(), filter.preview())
        } else {
            let projection = self.render_projection();
            format!(
                "db.{}.find({}, {})",
                collection.preview(),
                filter.preview(),
                projection.preview()
            )
        };

        // Add sort
        if !self.order_by.is_empty() {
            base_query = format!("{}.sort({})", base_query, self.order_by[0].0.preview());
        }

        // Add skip
        if let Some(skip) = self.skip {
            base_query = format!("{}.skip({})", base_query, skip);
        }

        // Add limit
        if let Some(limit) = self.limit {
            base_query = format!("{}.limit({})", base_query, limit);
        }

        expr!(base_query)
    }

    fn render_aggregate(&self) -> Expression {
        let collection = if let Some(ref source) = self.source {
            source.clone()
        } else {
            expr!("collection")
        };

        let mut pipeline = Vec::new();

        // Add $match stage for where conditions
        if !self.where_conditions.is_empty() {
            let match_condition = self.combine_where_conditions();
            pipeline.push(format!("{{$match: {}}}", match_condition.preview()));
        }

        // Add $project stage for fields
        if !self.fields.is_empty() {
            let project_doc = self.render_project_stage();
            pipeline.push(format!("{{$project: {}}}", project_doc.preview()));
        }

        // Add $sort stage
        if !self.order_by.is_empty() {
            pipeline.push(format!("{{$sort: {}}}", self.order_by[0].0.preview()));
        }

        // Add $skip stage
        if let Some(skip) = self.skip {
            pipeline.push(format!("{{$skip: {}}}", skip));
        }

        // Add $limit stage
        if let Some(limit) = self.limit {
            pipeline.push(format!("{{$limit: {}}}", limit));
        }

        let pipeline_str = format!("[{}]", pipeline.join(", "));
        expr!(format!(
            "db.{}.aggregate({})",
            collection.preview(),
            pipeline_str
        ))
    }

    fn render_project_stage(&self) -> Expression {
        if self.fields.is_empty() {
            return expr!("{}");
        }

        let mut project_fields = Vec::new();
        for field in &self.fields {
            match field {
                Field::Simple(name) => {
                    project_fields.push(format!("\"{}\": 1", name));
                }
                Field::Expression { expression, alias } => {
                    if let Some(alias) = alias {
                        project_fields.push(format!("\"{}\": {}", alias, expression.preview()));
                    } else {
                        project_fields.push(format!("\"field\": {}", expression.preview()));
                    }
                }
            }
        }

        expr!(format!("{{{}}}", project_fields.join(", ")))
    }

    fn combine_where_conditions(&self) -> Expression {
        if self.where_conditions.is_empty() {
            return expr!("{}");
        }

        if self.where_conditions.len() == 1 {
            return self.where_conditions[0].clone();
        }

        // For multiple conditions, just use the first one for now
        // In a complete implementation, you'd properly merge all conditions
        self.where_conditions[0].clone()
    }

    fn render_projection(&self) -> Expression {
        if self.fields.is_empty() {
            return expr!("{}");
        }

        if self.fields.len() == 1 {
            return self.fields[0].expression();
        }

        // For multiple fields, use the first one for now
        // In a complete implementation, you'd properly merge all fields
        self.fields[0].expression()
    }
}

#[async_trait]
impl Selectable for MongoSelect {
    fn set_source(&mut self, source: impl Into<Expr>, alias: Option<String>) {
        self.source = Some(expr!("{}", source.into()));
        self.source_alias = alias;
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(Field::new_simple(field.into()));
    }

    fn add_expression(&mut self, expression: Expression, alias: Option<String>) {
        self.fields.push(Field::new_expression(expression, alias));
    }

    fn add_where_condition(&mut self, condition: Expression) {
        self.where_conditions.push(condition);
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, field_or_expr: impl Into<Expr>, ascending: bool) {
        let expression = expr!("{}", field_or_expr.into());
        self.order_by.push((expression, ascending));
    }

    fn add_group_by(&mut self, expression: Expression) {
        self.group_by.push(expression);
    }

    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>) {
        self.limit = limit;
        self.skip = skip;
    }

    fn clear_fields(&mut self) {
        self.fields.clear();
    }

    fn clear_where_conditions(&mut self) {
        self.where_conditions.clear();
    }

    fn clear_order_by(&mut self) {
        self.order_by.clear();
    }

    fn clear_group_by(&mut self) {
        self.group_by.clear();
    }

    fn has_fields(&self) -> bool {
        !self.fields.is_empty()
    }

    fn has_where_conditions(&self) -> bool {
        !self.where_conditions.is_empty()
    }

    fn has_order_by(&self) -> bool {
        !self.order_by.is_empty()
    }

    fn has_group_by(&self) -> bool {
        !self.group_by.is_empty()
    }

    fn is_distinct(&self) -> bool {
        self.distinct
    }

    fn get_limit(&self) -> Option<i64> {
        self.limit
    }

    fn get_skip(&self) -> Option<i64> {
        self.skip
    }
}

impl Into<Expression> for MongoSelect {
    fn into(self) -> Expression {
        if self.has_expressions() {
            self.render_aggregate()
        } else {
            self.render_find()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use vantage_expressions::expr;

    #[test]
    fn test_basic_select() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("users"), None);
        let expr: Expression = select.into();
        assert_eq!(expr.preview(), "db.users.find({})");
    }

    #[test]
    fn test_select_with_filter() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("users"), None);
        select.add_where_condition(expr!("{\"name\": \"John\"}"));
        let expr: Expression = select.into();
        let result = expr.preview();
        assert!(result.contains("db.users.find("));
        assert!(result.contains("\"name\""));
        assert!(result.contains("\"John\""));
    }

    #[test]
    fn test_select_with_projection() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("users"), None);
        select.add_field("name".to_string());
        select.add_field("email".to_string());
        let expr: Expression = select.into();
        let result = expr.preview();
        assert!(result.contains("db.users.find({}, "));
        assert!(result.contains("name"));
    }

    #[test]
    fn test_select_with_sort() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("users"), None);
        select.add_order_by(expr!("{\"name\": 1}"), true);
        let expr: Expression = select.into();
        let result = expr.preview();
        assert!(result.contains("db.users.find({}).sort("));
        assert!(result.contains("\"name\""));
    }

    #[test]
    fn test_select_with_limit() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("users"), None);
        select.set_limit(Some(10), None);
        let expr: Expression = select.into();
        assert_eq!(expr.preview(), "db.users.find({}).limit(10)");
    }

    #[test]
    fn test_select_with_skip_and_limit() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("users"), None);
        select.set_limit(Some(10), Some(5));
        let expr: Expression = select.into();
        assert_eq!(expr.preview(), "db.users.find({}).skip(5).limit(10)");
    }

    #[test]
    fn test_select_trait_methods() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("users"), None);

        // Test trait methods
        select.add_where_condition(expr!("{\"age\": {\"$gt\": 18}}"));
        select.add_field("name".to_string());
        select.add_order_by(expr!("{\"name\": 1}"), true);
        select.set_limit(Some(10), Some(5));
        select.set_distinct(true);

        assert!(select.has_where_conditions());
        assert!(select.has_fields());
        assert!(select.has_order_by());
        assert!(select.is_distinct());
        assert_eq!(select.get_limit(), Some(10));
        assert_eq!(select.get_skip(), Some(5));
    }

    #[test]
    fn test_select_with_expression_field() {
        let mut select = MongoSelect::new();
        select.set_source(expr!("orders"), None);
        select.add_expression(expr!("quantity*price"), Some("total".to_string()));

        let expr: Expression = select.into();
        let result = expr.preview();

        assert!(result.contains("db.orders.aggregate([{$project: {\"total\": quantity*price}}])"));
    }

    #[test]
    fn test_find_vs_aggregate_rendering() {
        // Simple fields should use find
        let mut select_find = MongoSelect::new();
        select_find.set_source(expr!("users"), None);
        select_find.add_field("name".to_string());
        select_find.add_field("email".to_string());

        let expr: Expression = select_find.into();
        let result = expr.preview();
        assert!(result.contains("db.users.find("));
        assert!(!result.contains("aggregate"));

        // Expression fields should use aggregate
        let mut select_aggregate = MongoSelect::new();
        select_aggregate.set_source(expr!("orders"), None);
        select_aggregate.add_field("customer".to_string());
        select_aggregate.add_expression(expr!("quantity*price"), Some("total".to_string()));

        let expr: Expression = select_aggregate.into();
        let result = expr.preview();
        assert!(result.contains("db.orders.aggregate("));
        assert!(!result.contains("find"));
    }
}
