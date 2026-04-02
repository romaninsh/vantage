use std::sync::Arc;

use anyhow::{Result, anyhow};
use indexmap::IndexMap;
use serde_json::Value;
pub use with_traits::SqlQuery;

use crate::{
    expr, expr_arc,
    prelude::PgValueColumn,
    sql::{
        chunk::Chunk,
        expression::{Expression, ExpressionArc},
    },
    traits::column::SqlField,
};

mod parts;

pub use parts::*;

use super::table::TableAlias;

#[derive(Debug, Clone)]
pub struct Query {
    table: QuerySource,
    with: IndexMap<String, QuerySource>,
    distinct: bool,
    query_type: QueryType,
    fields: IndexMap<Option<String>, Arc<Box<dyn SqlField>>>,
    set_fields: IndexMap<String, Value>,

    where_conditions: QueryConditions,
    having_conditions: QueryConditions,
    joins: Vec<JoinQuery>,

    skip_items: Option<i64>,
    limit_items: Option<i64>,

    group_by: Vec<Expression>,
    order_by: Vec<Expression>,
}

#[derive(Debug)]
pub enum UniqAlias {
    FieldAlias,
    TableAlias,
}

impl Query {
    pub fn new() -> Query {
        Query {
            table: QuerySource::None,
            with: IndexMap::new(),
            distinct: false,
            query_type: QueryType::Select,
            fields: IndexMap::new(),

            set_fields: IndexMap::new(),

            where_conditions: QueryConditions::where_(),
            having_conditions: QueryConditions::having(),
            joins: Vec::new(),

            skip_items: None,
            limit_items: None,

            group_by: Vec::new(),
            order_by: Vec::new(),
        }
    }

    pub fn with_distinct(mut self) -> Self {
        self.set_distinct(true);
        self
    }

    pub fn with_table(mut self, table: &str, alias: Option<String>) -> Self {
        self.set_table(table, alias);
        self
    }

    pub fn with_with(mut self, alias: &str, subquery: Query) -> Self {
        self.add_with(
            alias.to_string(),
            QuerySource::Query(Arc::new(Box::new(subquery)), None),
        );
        self
    }

    pub fn with_source(mut self, source: QuerySource) -> Self {
        self.set_source(source);
        self
    }

    pub fn with_skip(mut self, skip: i64) -> Self {
        self.add_skip(Some(skip));
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.add_limit(Some(limit));
        self
    }

    pub fn with_skip_and_limit(mut self, skip: i64, limit: i64) -> Self {
        self.add_limit(Some(limit));
        self.add_skip(Some(skip));
        self
    }

    pub fn with_type(mut self, query_type: QueryType) -> Self {
        self.set_type(query_type);
        self
    }

    pub fn without_fields(mut self) -> Self {
        self.fields = IndexMap::new();
        self
    }

    pub fn with_field(mut self, name: String, field: impl SqlField + 'static) -> Self {
        self.add_field(Some(name), Arc::new(Box::new(field)));
        self
    }
    // Simplified ways to define a field with a string
    pub fn with_column_field(self, name: &str) -> Self {
        self.with_field(name.to_string(), Arc::new(PgValueColumn::new(name)))
    }

    pub fn with_field_arc(mut self, name: String, field: Arc<Box<dyn SqlField>>) -> Self {
        self.add_field(Some(name), field);
        self
    }

    pub fn with_where_condition(mut self, cond: Expression) -> Self {
        self.get_where_conditions_mut().add_condition(cond);
        self
    }

    pub fn with_having_condition(mut self, cond: Expression) -> Self {
        self.get_having_conditions_mut().add_condition(cond);
        self
    }

    pub fn with_join(mut self, join: JoinQuery) -> Self {
        self.add_join(join);
        self
    }

    pub fn with_condition(self, cond: impl Chunk + 'static) -> Self {
        self.with_where_condition(cond.render_chunk())
    }

    pub fn with_condition_arc(self, cond: Arc<Box<dyn Chunk>>) -> Self {
        self.with_where_condition(cond.render_chunk())
    }

    pub fn with_group_by(mut self, group_by: Expression) -> Self {
        self.add_group_by(group_by);
        self
    }

    pub fn with_order_by(mut self, order_by: Expression) -> Self {
        self.add_order_by(order_by);
        self
    }

    pub fn with_set_field(mut self, field: &str, value: Value) -> Self {
        self.set_field_value(field, value);
        self
    }

    fn render_with(&self) -> Expression {
        if self.with.is_empty() {
            Expression::empty()
        } else {
            let with = self
                .with
                .iter()
                .map(|(name, query)| {
                    expr_arc!(format!("{} AS {{}}", name), query.render_prefix("")).render_chunk()
                })
                .collect::<Vec<Expression>>();
            let e = Expression::from_vec(with, ", ");
            expr_arc!("WITH {} ", e).render_chunk()
        }
    }

    fn render_group_by(&self) -> Expression {
        if self.group_by.is_empty() {
            Expression::empty()
        } else {
            let group_by = Expression::from_vec(self.group_by.clone(), ", ");
            expr_arc!(" GROUP BY {}", group_by).render_chunk()
        }
    }

    fn render_order_by(&self) -> Expression {
        if self.order_by.is_empty() {
            Expression::empty()
        } else {
            let mut rev_vec = self.order_by.clone();
            rev_vec.reverse();
            let order_by = Expression::from_vec(rev_vec, ", ");
            expr_arc!(" ORDER BY {}", order_by).render_chunk()
        }
    }

    fn render_pagination(&self) -> Expression {
        if self.skip_items.is_none() && self.limit_items.is_none() {
            Expression::empty()
        } else {
            let mut rev_vec = Vec::new();
            if let Some(skip) = self.skip_items {
                rev_vec.push(expr!(" OFFSET {}::int4", skip));
            }
            if let Some(limit) = self.limit_items {
                rev_vec.push(expr!(" LIMIT {}::int4", limit));
            }
            Expression::from_vec(rev_vec, "")
        }
    }

    fn render_select(&self) -> Result<Expression> {
        let fields = if self.fields.len() > 0 {
            Expression::from_vec(
                self.fields
                    .iter()
                    .map(|f| {
                        f.1.render_column(f.0.as_ref().map(|s| s.as_str()))
                            .render_chunk()
                    })
                    .collect(),
                ", ",
            )
        } else {
            Expression::new("*".to_string(), vec![])
        };

        Ok(expr_arc!(
            format!(
                "{{}}SELECT{} {{}} {{}}{{}}{{}}{{}}{{}}{{}}{{}}",
                if self.distinct { " DISTINCT" } else { "" }
            ),
            self.render_with(),
            fields,
            self.table.render_chunk(),
            Expression::from_vec(self.joins.iter().map(|x| x.render_chunk()).collect(), ""),
            self.where_conditions.render_chunk(),
            self.render_group_by(),
            self.render_order_by(),
            self.render_pagination(),
            self.having_conditions.render_chunk()
        )
        .render_chunk())
    }

    fn render_insert(&self) -> Result<Expression> {
        let QuerySource::Table(table, _) = self.table.clone() else {
            return Err(anyhow!("Call set_table() for insert query"));
        };

        let fields = self
            .set_fields
            .iter()
            .map(|(k, _)| k.clone())
            .collect::<Vec<String>>()
            .join(", ");

        let values_str = self
            .set_fields
            .iter()
            .map(|_| "{}".to_string())
            .collect::<Vec<String>>()
            .join(", ");

        let values = self
            .set_fields
            .iter()
            .map(|(_, f)| f.clone())
            .collect::<Vec<Value>>();

        Ok(expr_arc!(
            format!(
                "{} INTO {} ({}) VALUES ({{}}) returning id",
                match self.query_type {
                    QueryType::Insert => "INSERT",
                    QueryType::Replace => "REPLACE",
                    _ => panic!("Invalid query type"),
                },
                table,
                fields
            ),
            Expression::new(values_str, values)
        )
        .render_chunk())
    }

    fn render_update(&self) -> Result<Expression> {
        let QuerySource::Table(table, _) = self.table.clone() else {
            return Err(anyhow!("Call set_table() for insert query"));
        };

        let set_fields = self
            .set_fields
            .iter()
            .map(|(k, v)| {
                let expr = expr_arc!(format!("{} = {{}}", k), v.clone());
                let boxed_chunk: Box<dyn Chunk> = Box::new(expr);
                Arc::new(boxed_chunk)
            })
            .collect::<Vec<Arc<Box<dyn Chunk>>>>();

        let set_fields = ExpressionArc::from_vec(set_fields, ", ");

        Ok(expr_arc!(
            format!("UPDATE {} SET {{}}{{}}", table),
            set_fields,
            self.where_conditions.render_chunk()
        )
        .render_chunk())
    }

    fn render_delete(&self) -> Result<Expression> {
        let QuerySource::Table(table, _) = self.table.clone() else {
            return Err(anyhow!("Call set_table() for insert query"));
        };

        Ok(expr_arc!(
            format!("DELETE FROM {}{{}}", table),
            self.where_conditions.render_chunk()
        )
        .render_chunk())
    }

    pub fn preview(&self) -> String {
        self.render_chunk().preview()
    }
}

impl Chunk for Query {
    fn render_chunk(&self) -> Expression {
        match &self.query_type {
            QueryType::Select => self.render_select(),
            QueryType::Insert | QueryType::Replace => self.render_insert(),
            QueryType::Update => self.render_update(),
            QueryType::Delete => self.render_delete(),
            QueryType::Expression(expr) => Ok(expr.clone()),
        }
        .unwrap()
    }
}

mod with_traits;
impl SqlQuery for Query {
    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }
    fn set_table(&mut self, table: &str, alias: Option<String>) {
        self.table = QuerySource::Table(table.to_string(), alias);
    }
    fn add_with(&mut self, alias: String, subquery: QuerySource) {
        self.with.insert(alias, subquery);
    }
    fn set_source(&mut self, source: QuerySource) {
        self.table = source;
    }
    fn set_type(&mut self, query_type: QueryType) {
        self.query_type = query_type;
    }
    fn add_field(&mut self, name: Option<String>, field: Arc<Box<dyn SqlField>>) {
        if self.fields.insert(name, field).is_some() {
            // panic!("Field is already defined");
            return;
        }
    }
    fn get_where_conditions_mut(&mut self) -> &mut QueryConditions {
        &mut self.where_conditions
    }
    fn get_having_conditions_mut(&mut self) -> &mut QueryConditions {
        &mut self.having_conditions
    }
    fn add_join(&mut self, join: JoinQuery) {
        self.joins.push(join);
    }
    fn add_group_by(&mut self, group_by: Expression) {
        self.group_by.push(group_by);
    }
    fn add_order_by(&mut self, order_by: Expression) {
        self.order_by.push(order_by);
    }
    fn add_limit(&mut self, limit: Option<i64>) {
        self.limit_items = limit;
    }
    fn add_skip(&mut self, skip: Option<i64>) {
        self.skip_items = skip;
    }
    fn set_field_value(&mut self, field: &str, value: Value) {
        match self.query_type {
            QueryType::Insert | QueryType::Update | QueryType::Replace => {
                self.set_fields.insert(field.to_string(), value);
            }
            _ => {
                panic!(
                    "Query should be \"Insert\", \"Update\" or \"Replace\" to set field value. Type is set to {:?}",
                    self.query_type
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{expr, sql::Operations};
    use serde_json::json;

    use super::*;

    #[test]
    fn test_where() {
        let expr1 = expr!("name = {}", "John");
        let expr2 = expr!("age").gt(30);

        let query = Query::new()
            .with_table("users", None)
            .with_condition(expr1)
            .with_condition(expr2);

        let wher = query.where_conditions.render_chunk();

        let (sql, params) = wher.render_chunk().split();

        assert_eq!(sql, " WHERE name = {} AND (age > {})");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], Value::String("John".to_string()));
        assert_eq!(params[1], Value::Number(30.into()));
    }

    #[test]
    fn test_select() {
        let (sql, params) = Query::new()
            .with_table("users", None)
            .with_column_field("id")
            .with_column_field("name")
            .with_field("calc".to_string(), expr_arc!("1 + 1"))
            .render_chunk()
            .split();

        assert_eq!(sql, "SELECT id, name, (1 + 1) AS calc FROM users");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_insert() {
        let (sql, params) = Query::new()
            .with_table("users", None)
            .with_type(QueryType::Insert)
            .with_set_field("name", "John".into())
            .with_set_field("surname", "Doe".into())
            .with_set_field("age", 30.into())
            .render_chunk()
            .split();

        assert_eq!(
            sql,
            "INSERT INTO users (name, surname, age) VALUES ({}, {}, {}) returning id"
        );
        assert_eq!(params.len(), 3);
        assert_eq!(params[0], json!("John"));
        assert_eq!(params[1], json!("Doe"));
        assert_eq!(params[2], json!(30));
    }

    #[test]
    fn test_update() {
        let (sql, params) = Query::new()
            .with_table("users", None)
            .with_type(QueryType::Update)
            .with_set_field("name", "John".into())
            .with_set_field("surname", "Doe".into())
            .with_set_field("age", 30.into())
            .with_condition(expr!("id = {}", 1))
            .render_chunk()
            .split();

        assert_eq!(
            sql,
            "UPDATE users SET name = {}, surname = {}, age = {} WHERE id = {}"
        );
        assert_eq!(params.len(), 4);
        assert_eq!(params[0], json!("John"));
        assert_eq!(params[1], json!("Doe"));
        assert_eq!(params[2], json!(30));
        assert_eq!(params[3], json!(1));
    }

    #[test]
    fn test_expression() {
        let (sql, params) = Query::new()
            .with_type(QueryType::Expression(expr!("CALL some_procedure()")))
            .render_chunk()
            .split();

        assert_eq!(sql, "CALL some_procedure()");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_expression_field() {
        let (sql, params) = Query::new()
            .with_table("product", None)
            .with_field("name_caps".to_string(), expr!("UPPER(name)"))
            .render_chunk()
            .split();

        assert_eq!(sql, "SELECT (UPPER(name)) AS name_caps FROM product");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_join_query() {
        let query = Query::new()
            .with_table("users", None)
            .with_column_field("id")
            .with_column_field("name");

        let join = JoinQuery::new(
            JoinType::Left,
            QuerySource::Table("roles".to_string(), None),
            QueryConditions::on().with_condition(expr!("users.role_id = roles.id")),
        );

        let (sql, params) = query.with_join(join).render_chunk().split();

        assert_eq!(
            sql,
            "SELECT id, name FROM users LEFT JOIN roles ON users.role_id = roles.id"
        );
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_render_with() {
        let roles = Query::new()
            .with_table("roles", None)
            .with_column_field("id")
            .with_column_field("role_name");

        let outer_query = Query::new()
            .with_table("users", None)
            .with_with("roles", roles)
            .with_join(JoinQuery::new(
                JoinType::Inner,
                QuerySource::Table("roles".to_string(), None),
                QueryConditions::on().with_condition(expr!("users.role_id = roles.id")),
            ))
            .with_column_field("user_name")
            .with_column_field("roles.role_name");

        let (sql, params) = outer_query.render_chunk().split();

        assert_eq!(
            sql,
            "WITH roles AS (SELECT id, role_name FROM roles) SELECT user_name, roles.role_name FROM users JOIN roles ON users.role_id = roles.id"
        );
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_group_and_order() {
        let query = Query::new()
            .with_table("users", None)
            .with_column_field("id")
            .with_column_field("name")
            .with_column_field("age")
            .with_group_by(expr!("name"))
            .with_order_by(expr!("age DESC"));

        let (sql, params) = query.render_chunk().split();

        assert_eq!(
            sql,
            "SELECT id, name, age FROM users GROUP BY name ORDER BY age DESC"
        );
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_render_pagination() {
        let query = Query::new()
            .with_table("users", None)
            .with_column_field("id")
            .with_column_field("name")
            .with_column_field("age")
            .with_skip_and_limit(10, 20);

        let (sql, params) = query.render_pagination().render_chunk().split();

        assert_eq!(sql, " OFFSET {}::int4 LIMIT {}::int4");
        assert_eq!(params.len(), 2);
        assert_eq!(
            query.render_pagination().preview(),
            " OFFSET 10::int4 LIMIT 20::int4"
        );
        assert_eq!(
            expr_arc!("SELECT x{}", query.render_pagination())
                .render_chunk()
                .preview(),
            "SELECT x OFFSET 10::int4 LIMIT 20::int4"
        );
    }

    #[test]
    fn test_limit() {
        let query = Query::new()
            .with_table("users", None)
            .with_column_field("id")
            .with_column_field("name")
            .with_column_field("age")
            .with_limit(20);

        let (sql, params) = query.render_chunk().split();

        assert_eq!(sql, "SELECT id, name, age FROM users LIMIT {}::int4");
        assert_eq!(params.len(), 1);
        assert_eq!(
            query.render_chunk().preview(),
            "SELECT id, name, age FROM users LIMIT 20::int4"
        );
    }

    #[test]
    fn test_skip() {
        let query = Query::new()
            .with_table("users", None)
            .with_column_field("id")
            .with_column_field("name")
            .with_column_field("age")
            .with_skip(10);

        assert_eq!(
            query.render_chunk().preview(),
            "SELECT id, name, age FROM users OFFSET 10::int4"
        );
    }

    #[test]
    fn test_skip_and_limit() {
        let mut query = Query::new()
            .with_table("users", None)
            .with_column_field("id")
            .with_column_field("name")
            .with_column_field("age");
        query.add_skip(Some(10));
        query.add_limit(Some(20));

        assert_eq!(
            query.render_chunk().preview(),
            "SELECT id, name, age FROM users OFFSET 10::int4 LIMIT 20::int4"
        );
    }
}
