pub mod expressive;
pub mod join_query;
pub mod query_conditions;
pub mod query_source;
pub mod query_type;

use indexmap::IndexMap;
use serde_json::Value;
use vantage_expressions::{OwnedExpression, expr, protocol::selectable::Selectable};

use crate::Identifier;
use join_query::JoinQuery;
use query_conditions::QueryConditions;
use query_source::QuerySource;
use query_type::QueryType;

#[derive(Debug, Clone)]
pub struct Select {
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

    group_by: Vec<OwnedExpression>,
    order_by: Vec<OwnedExpression>,
}

impl Select {
    pub fn new() -> Self {
        Self {
            from: Vec::new(),
            with: IndexMap::new(),
            distinct: false,
            query_type: QueryType,
            fields: IndexMap::new(),
            set_fields: IndexMap::new(),
            where_conditions: QueryConditions::new(),
            having_conditions: QueryConditions::new(),
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

    fn render_where(&self) -> OwnedExpression {
        self.where_conditions.render()
    }

    fn render_group_by(&self) -> OwnedExpression {
        if self.group_by.is_empty() {
            expr!("")
        } else {
            let group_expressions: Vec<OwnedExpression> = self.group_by.clone();
            expr!(
                " GROUP BY {}",
                OwnedExpression::from_vec(group_expressions, ", ")
            )
        }
    }

    fn render_order_by(&self) -> OwnedExpression {
        if self.order_by.is_empty() {
            expr!("")
        } else {
            let mut result = Vec::new();
            for chunk in self.order_by.chunks(2) {
                if chunk.len() == 2 {
                    let direction = match chunk[1].preview().as_str() {
                        "true" => "ASC",
                        _ => "DESC",
                    };
                    result.push(expr!("{} {}", chunk[0].clone(), direction));
                } else {
                    result.push(chunk[0].clone());
                }
            }

            expr!(" ORDER BY {}", OwnedExpression::from_vec(result, ", "))
        }
    }

    fn render_limit(&self) -> OwnedExpression {
        match (self.limit_items, self.skip_items) {
            (Some(limit), Some(skip)) => expr!(" LIMIT {} OFFSET {}", limit, skip),
            (Some(limit), None) => expr!(" LIMIT {}", limit),
            (None, Some(skip)) => expr!(" OFFSET {}", skip),
            (None, None) => expr!(""),
        }
    }

    fn render_distinct(&self) -> OwnedExpression {
        if self.distinct {
            expr!("DISTINCT ")
        } else {
            expr!("")
        }
    }
}

impl Into<OwnedExpression> for Select {
    fn into(self) -> OwnedExpression {
        let distinct = self.render_distinct();
        let fields = self.render_fields();

        let mut query = if distinct.preview().is_empty() {
            expr!("SELECT {}", fields)
        } else {
            expr!("SELECT {}{}", distinct, fields)
        };

        let from_clause = self.render_from();
        if !from_clause.preview().is_empty() {
            query = expr!("{}{}", query, from_clause);
        }

        let where_clause = self.render_where();
        if !where_clause.preview().is_empty() {
            query = expr!("{}{}", query, where_clause);
        }

        let group_by_clause = self.render_group_by();
        if !group_by_clause.preview().is_empty() {
            query = expr!("{}{}", query, group_by_clause);
        }

        let order_by_clause = self.render_order_by();
        if !order_by_clause.preview().is_empty() {
            query = expr!("{}{}", query, order_by_clause);
        }

        let limit_clause = self.render_limit();
        if !limit_clause.preview().is_empty() {
            query = expr!("{}{}", query, limit_clause);
        }

        query
    }
}

impl Selectable for Select {
    fn set_source(&mut self, source: OwnedExpression, alias: Option<String>) {
        let mut query_source = QuerySource::new(source);
        if let Some(alias) = alias {
            query_source = query_source.with_alias(alias);
        }
        self.from = vec![query_source];
    }

    fn add_field(&mut self, field: String) {
        self.fields.insert(None, Identifier::new(field).into());
    }

    fn add_expression(&mut self, expression: OwnedExpression, alias: Option<String>) {
        self.fields.insert(alias, expression);
    }

    fn add_where_condition(&mut self, condition: OwnedExpression) {
        self.where_conditions.add_condition(condition);
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, expression: OwnedExpression, ascending: bool) {
        self.order_by.push(expression);
        self.order_by
            .push(expr!(if ascending { "true" } else { "false" }));
    }

    fn add_group_by(&mut self, expression: OwnedExpression) {
        self.group_by.push(expression);
    }

    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>) {
        self.limit_items = limit;
        self.skip_items = skip;
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
        self.where_conditions.has_conditions()
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
        self.limit_items
    }

    fn get_skip(&self) -> Option<i64> {
        self.skip_items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_select() {
        let select = Select::new();
        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT *");
    }

    #[test]
    fn test_select_with_fields() {
        let mut fields = IndexMap::new();
        fields.insert(Some("name".to_string()), Identifier::new("name").into());
        fields.insert(Some("age".to_string()), Identifier::new("age").into());

        let select = Select::new().fields(fields);
        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name` AS `name`, `age` AS `age`");
    }

    #[test]
    fn test_select_with_from() {
        let select = Select::new().from(vec![QuerySource::new(Identifier::new("users"))]);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT * FROM `users`");
    }

    #[test]
    fn test_select_with_fields_and_from() {
        let mut fields = IndexMap::new();
        fields.insert(Some("name".to_string()), Identifier::new("name").into());
        fields.insert(Some("age".to_string()), Identifier::new("age").into());

        let select = Select::new()
            .fields(fields)
            .from(vec![QuerySource::new(Identifier::new("users"))]);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT `name` AS `name`, `age` AS `age` FROM `users`");
    }

    #[test]
    fn test_selectable_trait() {
        let mut select = Select::new();

        select.set_source(expr!("users"), None);
        select.add_field("name".to_string());
        select.add_expression(expr!("age"), Some("user_age".to_string()));
        select.add_where_condition(expr!("age > 18"));
        select.set_distinct(true);
        select.add_order_by(expr!("name"), true);
        select.add_group_by(expr!("department"));
        select.set_limit(Some(10), Some(5));

        assert!(select.has_fields());
        assert!(select.has_where_conditions());
        assert!(select.has_order_by());
        assert!(select.has_group_by());
        assert!(select.is_distinct());
        assert_eq!(select.get_limit(), Some(10));
        assert_eq!(select.get_skip(), Some(5));
    }
}
