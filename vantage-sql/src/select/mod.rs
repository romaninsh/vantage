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
pub use query_source::QuerySource;
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

    // Fluent builder methods
    pub fn with_distinct(mut self) -> Self {
        self.distinct = true;
        self
    }

    pub fn with_table(mut self, table: &str) -> Self {
        self.from = vec![QuerySource::table(table)];
        self
    }

    pub fn with_table_alias(mut self, table: &str, alias: &str) -> Self {
        self.from = vec![QuerySource::table_with_alias(table, alias)];
        self
    }

    pub fn with_source(mut self, source: QuerySource) -> Self {
        self.from = vec![source];
        self
    }

    pub fn with_field(mut self, name: &str, expression: OwnedExpression) -> Self {
        self.fields.insert(Some(name.to_string()), expression);
        self
    }

    pub fn with_column(mut self, name: &str) -> Self {
        self.fields
            .insert(Some(name.to_string()), Identifier::new(name).into());
        self
    }

    pub fn with_expression(mut self, expression: OwnedExpression, alias: &str) -> Self {
        self.fields.insert(Some(alias.to_string()), expression);
        self
    }

    pub fn with_where_condition(mut self, condition: OwnedExpression) -> Self {
        self.where_conditions.add_condition(condition);
        self
    }

    pub fn with_having_condition(mut self, condition: OwnedExpression) -> Self {
        self.having_conditions.add_condition(condition);
        self
    }

    pub fn with_join(mut self, join: JoinQuery) -> Self {
        self.joins.push(join);
        self
    }

    pub fn with_group_by(mut self, expression: OwnedExpression) -> Self {
        self.group_by.push(expression);
        self
    }

    pub fn with_order_by(mut self, expression: OwnedExpression) -> Self {
        self.order_by.push(expression);
        self.order_by.push(expr!("true")); // Default to ascending
        self
    }

    pub fn with_order_by_desc(mut self, expression: OwnedExpression) -> Self {
        self.order_by.push(expression);
        self.order_by.push(expr!("false")); // Descending
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.limit_items = Some(limit);
        self
    }

    pub fn with_skip(mut self, skip: i64) -> Self {
        self.skip_items = Some(skip);
        self
    }

    pub fn with_skip_and_limit(mut self, skip: i64, limit: i64) -> Self {
        self.skip_items = Some(skip);
        self.limit_items = Some(limit);
        self
    }

    pub fn with_with(mut self, alias: &str, subquery: Select) -> Self {
        self.with
            .insert(alias.to_string(), QuerySource::query(subquery));
        self
    }

    pub fn without_fields(mut self) -> Self {
        self.fields.clear();
        self
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

    fn render_with(&self) -> OwnedExpression {
        if self.with.is_empty() {
            expr!("")
        } else {
            let with_expressions: Vec<OwnedExpression> = self
                .with
                .iter()
                .map(|(alias, source)| match source {
                    QuerySource::Query(query, _) => {
                        let subquery: OwnedExpression = query.as_ref().as_ref().clone().into();
                        expr!("{} AS ({})", Identifier::new(alias), subquery)
                    }
                    _ => {
                        let source_expr: OwnedExpression = source.clone().into();
                        expr!("{} AS ({})", Identifier::new(alias), source_expr)
                    }
                })
                .collect();
            expr!(
                "WITH {} ",
                OwnedExpression::from_vec(with_expressions, ", ")
            )
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

    fn render_having(&self) -> OwnedExpression {
        if self.having_conditions.has_conditions() {
            let conditions = self.having_conditions.render_conditions();
            expr!(" HAVING {}", conditions)
        } else {
            expr!("")
        }
    }

    fn render_joins(&self) -> OwnedExpression {
        if self.joins.is_empty() {
            expr!("")
        } else {
            let join_expressions: Vec<OwnedExpression> =
                self.joins.iter().map(|join| join.render()).collect();
            OwnedExpression::from_vec(join_expressions, "")
        }
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
                    let is_ascending = chunk[1].preview().as_str() != "false";
                    if is_ascending {
                        result.push(expr!("{} ASC", chunk[0].clone()));
                    } else {
                        result.push(expr!("{} DESC", chunk[0].clone()));
                    }
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
        let with_clause = self.render_with();
        let distinct = self.render_distinct();
        let fields = self.render_fields();

        let mut query = if distinct.preview().is_empty() {
            expr!("SELECT {}", fields)
        } else {
            expr!("SELECT {}{}", distinct, fields)
        };

        // Add WITH clause at the beginning
        if !with_clause.preview().is_empty() {
            query = expr!("{}{}", with_clause, query);
        }

        let from_clause = self.render_from();
        if !from_clause.preview().is_empty() {
            query = expr!("{}{}", query, from_clause);
        }

        let joins_clause = self.render_joins();
        if !joins_clause.preview().is_empty() {
            query = expr!("{}{}", query, joins_clause);
        }

        let where_clause = self.render_where();
        if !where_clause.preview().is_empty() {
            query = expr!("{}{}", query, where_clause);
        }

        let group_by_clause = self.render_group_by();
        if !group_by_clause.preview().is_empty() {
            query = expr!("{}{}", query, group_by_clause);
        }

        let having_clause = self.render_having();
        if !having_clause.preview().is_empty() {
            query = expr!("{}{}", query, having_clause);
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

    #[test]
    fn test_fluent_builder_methods() {
        let select = Select::new()
            .with_table("users")
            .with_column("id")
            .with_column("name")
            .with_expression(expr!("1 + 1"), "calc")
            .with_where_condition(expr!("name = 'John'"))
            .with_where_condition(expr!("age > 30"))
            .with_distinct();

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains("SELECT DISTINCT"));
        assert!(sql.contains("FROM `users`"));
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("name = 'John'"));
        assert!(sql.contains("age > 30"));
    }

    #[test]
    fn test_join_query() {
        let select = Select::new()
            .with_table("users")
            .with_column("id")
            .with_column("name")
            .with_join(
                JoinQuery::left(QuerySource::table("roles")).on(expr!("users.role_id = roles.id")),
            );

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains(
            "SELECT `id` AS `id`, `name` AS `name` FROM `users` LEFT JOIN `roles` ON users.role_id = roles.id"
        ));
    }

    #[test]
    fn test_with_subquery() {
        let roles = Select::new()
            .with_table("roles")
            .with_column("id")
            .with_column("role_name");

        let outer_query = Select::new()
            .with_table("users")
            .with_with("roles", roles)
            .with_join(
                JoinQuery::inner(QuerySource::table("roles")).on(expr!("users.role_id = roles.id")),
            )
            .with_column("user_name")
            .with_field("roles.role_name", expr!("roles.role_name"));

        let expr: OwnedExpression = outer_query.into();
        let sql = expr.preview();

        assert!(sql.contains(
            "WITH `roles` AS (SELECT `id` AS `id`, `role_name` AS `role_name` FROM `roles`)"
        ));
        assert!(sql.contains("JOIN `roles` ON users.role_id = roles.id"));
    }

    #[test]
    fn test_group_and_order() {
        let select = Select::new()
            .with_table("users")
            .with_column("id")
            .with_column("name")
            .with_column("age")
            .with_group_by(expr!("name"))
            .with_order_by_desc(expr!("age"));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert!(sql.contains("GROUP BY name"));
        assert!(sql.contains("ORDER BY age DESC"));
    }

    #[test]
    fn test_pagination() {
        let select = Select::new()
            .with_table("users")
            .with_column("id")
            .with_column("name")
            .with_column("age")
            .with_skip_and_limit(10, 20);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains("LIMIT 20 OFFSET 10"));
    }

    #[test]
    fn test_limit_only() {
        let select = Select::new()
            .with_table("users")
            .with_column("id")
            .with_column("name")
            .with_column("age")
            .with_limit(20);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains("LIMIT 20"));
        assert!(!sql.contains("OFFSET"));
    }

    #[test]
    fn test_skip_only() {
        let select = Select::new()
            .with_table("users")
            .with_column("id")
            .with_column("name")
            .with_column("age")
            .with_skip(10);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains("OFFSET 10"));
        assert!(!sql.contains("LIMIT"));
    }

    #[test]
    fn test_having_conditions() {
        let select = Select::new()
            .with_table("users")
            .with_column("department")
            .with_expression(expr!("COUNT(*)"), "total")
            .with_group_by(expr!("department"))
            .with_having_condition(expr!("COUNT(*) > 5"));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains("GROUP BY department"));
        assert!(sql.contains("HAVING COUNT(*) > 5"));
    }

    #[test]
    fn test_table_with_alias() {
        let select = Select::new()
            .with_table_alias("users", "u")
            .with_column("id")
            .with_column("name");

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains("FROM `users` AS `u`"));
    }

    #[test]
    fn test_expression_field() {
        let select = Select::new()
            .with_table("product")
            .with_field("name_caps", expr!("UPPER(name)"));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert_eq!(sql, "SELECT UPPER(name) AS `name_caps` FROM `product`");
    }

    #[test]
    fn test_multiple_conditions() {
        let select = Select::new()
            .with_table("users")
            .with_column("name")
            .with_where_condition(expr!("name = 'John'"))
            .with_where_condition(expr!("age > 30"));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains("WHERE (name = 'John') AND (age > 30)"));
    }

    #[test]
    fn test_complex_join_with_multiple_conditions() {
        let select = Select::new()
            .with_table("users")
            .with_column("name")
            .with_join(
                JoinQuery::inner(QuerySource::table("orders"))
                    .on(expr!("users.id = orders.user_id"))
                    .on(expr!("orders.status = 'active'")),
            );

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();
        assert!(sql.contains(
            "JOIN `orders` ON (users.id = orders.user_id) AND (orders.status = 'active')"
        ));
    }
}
