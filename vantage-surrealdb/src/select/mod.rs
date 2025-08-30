//! # SurrealDB Select Query Builder
//!
//! Builds SELECT query for SurrealDB. Implements [`Selectable`] protocol.

// pub mod expressive;
pub mod field;
pub mod select_field;
pub mod target;

use std::marker::PhantomData;

use field::Field;
use select_field::SelectField;
use serde_json::Value;
use target::Target;

use crate::{
    SurrealDB,
    identifier::Identifier,
    operation::Expressive,
    sum::{Fx, Sum},
    surreal_return::SurrealReturn,
};
use vantage_expressions::{
    DataSource, Expr, OwnedExpression, expr,
    protocol::selectable::Selectable,
    result::{self, QueryResult},
};

/// SurrealDB SELECT query builder
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_expressions::{expr, protocol::selectable::Selectable};
/// use vantage_surrealdb::select::SurrealSelect;
///
/// // doc wip
/// let mut select = SurrealSelect::new();
/// select.set_source("users", None);
/// select.add_field("name".to_string());
/// ```
#[derive(Debug, Clone)]
pub struct SurrealSelect<T = result::Rows> {
    pub fields: Vec<SelectField>, // SELECT clause fields
    pub fields_omit: Vec<Field>,
    single_value: bool,
    from_only: bool,
    pub from: Vec<Target>, // FROM clause targets
    pub from_omit: bool,
    pub where_conditions: Vec<OwnedExpression>,
    pub order_by: Vec<(OwnedExpression, bool)>,
    pub group_by: Vec<OwnedExpression>,
    pub distinct: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
    _phantom: PhantomData<T>,
}

impl SurrealSelect<result::Single> {
    pub async fn get(&self, db: &SurrealDB) -> Value {
        match db.execute(&self.expr()).await {
            Value::Array(arr) if !arr.is_empty() => arr[0].clone(),
            other => other,
        }
    }
}

impl SurrealSelect<result::List> {
    pub async fn get(&self, db: &SurrealDB) -> Vec<Value> {
        match db.execute(&self.expr()).await {
            Value::Array(list) => list,
            _ => panic!("Expected array from database query"),
        }
    }
}

impl SurrealSelect<result::Rows> {
    pub async fn get(&self, db: &SurrealDB) -> Vec<serde_json::Map<String, Value>> {
        match db.execute(&self.expr()).await {
            Value::Array(rows) => rows
                .into_iter()
                .map(|item| match item {
                    Value::Object(map) => map,
                    _ => panic!("Expected object in result set"),
                })
                .collect(),
            _ => panic!("Expected array from database query"),
        }
    }
}

impl SurrealSelect<result::SingleRow> {
    pub async fn get(&self, db: &SurrealDB) -> serde_json::Map<String, Value> {
        match db.execute(&self.expr()).await {
            Value::Array(arr) if !arr.is_empty() => match &arr[0] {
                Value::Object(map) => map.clone(),
                _ => panic!("Expected object in result set"),
            },
            Value::Object(map) => map,
            _ => panic!("Expected object or array from database query"),
        }
    }
}

impl<T> Default for SurrealSelect<T> {
    fn default() -> Self {
        Self {
            fields: Vec::new(),
            fields_omit: Vec::new(),
            single_value: false,
            from: Vec::new(),
            from_omit: false,
            from_only: false,
            where_conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            distinct: false,
            limit: None,
            skip: None,
            _phantom: PhantomData,
        }
    }
}

impl SurrealSelect {
    /// Creates a new SELECT query builder
    ///
    /// doc wip
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the fields for the SELECT clause
    ///
    /// doc wip
    pub fn fields(mut self, fields: Vec<SelectField>) -> Self {
        self.fields = fields;
        self
    }

    /// Sets the FROM clause targets
    ///
    /// doc wip
    pub fn from(mut self, targets: Vec<Target>) -> Self {
        self.from = targets;
        self
    }

    pub fn with_source(mut self, source: impl Into<Expr>) -> Self {
        self.set_source(source, None);
        self
    }
    pub fn with_source_as(mut self, source: impl Into<Expr>, alias: impl Into<String>) -> Self {
        self.set_source(source, Some(alias.into()));
        self
    }

    pub fn with_condition(mut self, condition: OwnedExpression) -> Self {
        self.add_where_condition(condition);
        self
    }

    pub fn with_order(mut self, field_or_expr: impl Into<Expr>, ascending: bool) -> Self {
        self.add_order_by(field_or_expr, ascending);
        self
    }
}

impl<T: QueryResult> SurrealSelect<T> {
    pub fn without_fields(mut self) -> Self {
        self.fields = vec![];
        self
    }
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        Selectable::add_field(&mut self, field);
        self
    }
    pub fn with_expression(mut self, expression: OwnedExpression, alias: Option<String>) -> Self {
        Selectable::add_expression(&mut self, expression, alias);
        self
    }
    pub fn with_value(mut self) -> Self {
        self.single_value = true;
        self
    }
    /// Renders the SELECT fields clause
    ///
    /// doc wip
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

    /// Renders the FROM clause
    ///
    /// doc wip
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
                format!(" FROM {}{{}}", if self.from_only { "ONLY " } else { "" }),
                OwnedExpression::from_vec(from_expressions, ", ")
            )
        }
    }

    /// Renders the WHERE clause
    ///
    /// doc wip
    fn render_where(&self) -> OwnedExpression {
        if self.where_conditions.is_empty() {
            expr!("")
        } else {
            // Combine multiple conditions with AND
            let combined = OwnedExpression::from_vec(self.where_conditions.clone(), " AND ");
            expr!(" WHERE {}", combined)
        }
    }

    /// Renders the GROUP BY clause
    ///
    /// doc wip
    fn render_group_by(&self) -> OwnedExpression {
        if self.group_by.is_empty() {
            expr!("")
        } else {
            let group_expressions: Vec<OwnedExpression> = self.group_by.iter().cloned().collect();
            expr!(
                " GROUP BY {}",
                OwnedExpression::from_vec(group_expressions, ", ")
            )
        }
    }

    /// Renders the ORDER BY clause
    ///
    /// doc wip
    fn render_order_by(&self) -> OwnedExpression {
        if self.order_by.is_empty() {
            expr!("")
        } else {
            let order_expressions: Vec<OwnedExpression> = self
                .order_by
                .iter()
                .map(|(expression, ascending)| {
                    if *ascending {
                        expr!("{}", expression.clone())
                    } else {
                        expr!("{} DESC", expression.clone())
                    }
                })
                .collect();
            let combined = OwnedExpression::from_vec(order_expressions, ", ");
            expr!(" ORDER BY {}", combined)
        }
    }

    /// Renders the LIMIT and START clauses
    ///
    /// doc wip
    fn render_limit(&self) -> OwnedExpression {
        match (self.limit, self.skip) {
            (Some(limit), Some(skip)) => expr!(" LIMIT {} START {}", limit, skip),
            (Some(limit), None) => expr!(" LIMIT {}", limit),
            (None, Some(skip)) => expr!(" START {}", skip),
            (None, None) => expr!(""),
        }
    }

    /// Renders entire statement into an expression
    fn render(&self) -> OwnedExpression {
        expr!(
            "SELECT {}{}{}{}{}{}{}",
            if self.single_value {
                expr!("VALUE ")
            } else {
                expr!("")
            },
            self.render_fields(),
            self.render_from(),
            self.render_where(),
            self.render_group_by(),
            self.render_order_by(),
            self.render_limit()
        )
    }

    /// Renders everything into a string. Use for
    /// debug only. Never or use as part of another query!!
    pub fn preview(&self) -> String {
        self.render().preview()
    }
}

impl<T: QueryResult> Expressive for SurrealSelect<T> {
    fn expr(&self) -> OwnedExpression {
        self.render()
    }
}

// impl<T: QueryResult> Into<OwnedExpression> for SurrealSelect<T> {
//     fn into(self) -> OwnedExpression {
//         self.render()
//     }
// }

impl<T: QueryResult> Into<OwnedExpression> for SurrealSelect<T> {
    fn into(self) -> OwnedExpression {
        self.render()
    }
}

impl<T: QueryResult> Selectable for SurrealSelect<T> {
    fn set_source(&mut self, source: impl Into<Expr>, _alias: Option<String>) {
        let source_expr = match source.into() {
            Expr::Scalar(Value::String(s)) => Identifier::new(s).into(),
            other => expr!("({})", other),
        };
        self.from = vec![Target::new(source_expr)];
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(SelectField::new(Identifier::new(field)));
    }

    fn add_expression(&mut self, expression: OwnedExpression, alias: Option<String>) {
        let mut field = SelectField::new(expression);
        if let Some(alias) = alias {
            field = field.with_alias(alias);
        }
        self.fields.push(field);
    }

    fn add_where_condition(&mut self, condition: OwnedExpression) {
        self.where_conditions.push(condition);
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, field_or_expr: impl Into<Expr>, ascending: bool) {
        let expression = match field_or_expr.into() {
            Expr::Scalar(Value::String(s)) => Identifier::new(s).into(),
            other => expr!("{}", other),
        };

        self.order_by.push((expression, ascending));
    }

    fn add_group_by(&mut self, expression: OwnedExpression) {
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

impl SurrealSelect<result::Rows> {
    pub fn as_sum(self, field_or_expr: impl Into<Expr>) -> SurrealReturn {
        let query = self.without_fields();
        let query = match field_or_expr.into() {
            Expr::Scalar(Value::String(s)) => query.only_column(s),
            Expr::Nested(e) => query.only_expression(e),
            other => query.only_expression(expr!("{}", other)),
        };

        SurrealReturn::new(Sum::new(query.expr()).into()).into()
    }
    pub fn as_count(self) -> SurrealReturn {
        let result = self.only_expression(expr!("*"));
        SurrealReturn::new(Fx::new("count", vec![result.expr()]).into()).into()
    }
    pub fn only_expression(self, expr: OwnedExpression) -> SurrealSelect<result::List> {
        self.without_fields().with_expression(expr, None).as_list()
    }
    pub fn only_column(self, column: impl Into<String>) -> SurrealSelect<result::List> {
        self.without_fields().with_field(column).as_list()
    }
    fn as_list(self) -> SurrealSelect<result::List> {
        if self.from_only {
            panic!("SelectQuery<Rows>::as_list() must not have from_only=true");
        }
        if self.single_value {
            panic!("SelectQuery<Rows>::as_list() must not have single_value=true");
        }
        SurrealSelect {
            fields: self.fields,
            fields_omit: self.fields_omit,
            from: self.from,
            from_omit: self.from_omit,
            from_only: self.from_only,
            where_conditions: self.where_conditions,
            order_by: self.order_by,
            group_by: self.group_by,
            distinct: self.distinct,
            limit: self.limit,
            skip: self.skip,
            _phantom: PhantomData,

            // Use "VALUE" for column
            single_value: true,
        }
    }
    pub fn only_first_row(self) -> SurrealSelect<result::SingleRow> {
        if self.from_only {
            panic!("SelectQuery<Rows>::as_one_row() must not have from_only=true");
        }
        if self.single_value {
            panic!("SelectQuery<Rows>::as_one_row() must not have single_value=true");
        }
        SurrealSelect {
            fields: self.fields,
            fields_omit: self.fields_omit,
            from: self.from,
            from_omit: self.from_omit,
            from_only: true,
            where_conditions: self.where_conditions,
            order_by: self.order_by,
            group_by: self.group_by,
            distinct: self.distinct,
            limit: self.limit,
            skip: self.skip,
            _phantom: PhantomData,

            // Use normal row
            single_value: self.single_value,
        }
    }
}
impl SurrealSelect<result::SingleRow> {
    pub fn only_expression(self, expr: OwnedExpression) -> SurrealSelect<result::Single> {
        self.without_fields()
            .with_expression(expr, None)
            .as_single_value()
    }
    pub fn only_column(self, column: impl Into<String>) -> SurrealSelect<result::Single> {
        self.without_fields().with_field(column).as_single_value()
    }
    pub fn as_single_value(self) -> SurrealSelect<result::Single> {
        if !self.from_only {
            panic!("SelectQuery<SingleRow>::as_single_value() must have from_only=true");
        }
        if self.single_value {
            panic!("SelectQuery<SingleRow>::as_single_value() must not have single_value=true");
        }
        SurrealSelect {
            fields: self.fields,
            fields_omit: self.fields_omit,
            from: self.from,
            from_omit: self.from_omit,
            from_only: true,
            where_conditions: self.where_conditions,
            order_by: self.order_by,
            group_by: self.group_by,
            distinct: self.distinct,
            limit: self.limit,
            skip: self.skip,
            _phantom: PhantomData,

            // Use "VALUE" for single value
            single_value: true,
        }
    }
    // pub fn as_list(self, field_or_expr: impl Into<Expr>) -> SurrealSelect<result::List> {
    //     let mut result = self.into_list();
    //     match field_or_expr.into() {
    //         Expr::Scalar(Value::String(s)) => result.add_field(s),
    //         Expr::Nested(e) => result.add_expression(e, None),
    //         other => result.add_expression(expr!("{}", other), None),
    //     };
    //     result
    // }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::field::Field;
    use crate::select::select_field::SelectField;
    use crate::select::target::Target;

    #[test]
    fn test_basic_select() {
        let select = SurrealSelect::new()
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
        let select = SurrealSelect::new().from(vec![Target::new(expr!("users"))]);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT * FROM users");
    }

    #[test]
    fn test_select_with_where_condition() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_where_condition(expr!("age > 18"));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT name FROM users WHERE age > 18");
    }

    #[test]
    fn test_select_with_multiple_where_conditions() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_where_condition(expr!("age > 18"));
        select.add_where_condition(expr!("active = true"));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(
            sql,
            "SELECT name FROM users WHERE age > 18 AND active = true"
        );
    }

    #[test]
    fn test_select_with_order_by() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_order_by(expr!("name"), true);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT name FROM users ORDER BY name");
    }

    #[test]
    fn test_select_with_order_by_desc() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_order_by(expr!("created_at"), false);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT name FROM users ORDER BY created_at DESC");
    }

    #[test]
    fn test_select_with_group_by() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("department".to_string());
        select.add_expression(expr!("count()"), Some("count".to_string()));
        select.add_group_by(expr!("department"));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(
            sql,
            "SELECT department, count() AS count FROM users GROUP BY department"
        );
    }

    #[test]
    fn test_select_with_limit() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.set_limit(Some(10), None);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT name FROM users LIMIT 10");
    }

    #[test]
    fn test_select_with_limit_and_start() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.set_limit(Some(10), Some(20));

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(sql, "SELECT name FROM users LIMIT 10 START 20");
    }

    #[test]
    fn test_complex_select_query() {
        let mut select = SurrealSelect::new();
        select.set_source("orders", None);
        select.add_field("customer_id".to_string());
        select.add_expression(expr!("SUM(total)"), Some("total_amount".to_string()));
        select.add_where_condition(expr!("status = 'completed'"));
        select.add_group_by(expr!("customer_id"));
        select.add_order_by(expr!("total_amount"), false);
        select.set_limit(Some(5), None);

        let expr: OwnedExpression = select.into();
        let sql = expr.preview();

        assert_eq!(
            sql,
            "SELECT customer_id, SUM(total) AS total_amount FROM orders WHERE status = 'completed' GROUP BY customer_id ORDER BY total_amount DESC LIMIT 5"
        );
    }

    #[test]
    fn test_selectable_trait_methods() {
        let mut select = SurrealSelect::new();

        // Test Selectable trait methods
        select.set_source(expr!("users"), None);
        select.add_field("name".to_string());
        select.add_field("email".to_string());
        select.add_expression(expr!("age * 2"), Some("double_age".to_string()));
        select.add_where_condition(expr!("age > 18"));
        select.add_order_by(expr!("name"), true);
        select.add_group_by(expr!("department"));
        select.set_limit(Some(10), Some(5));
        select.set_distinct(true);

        // Test trait query methods
        assert!(select.has_fields());
        assert!(select.has_where_conditions());
        assert!(select.has_order_by());
        assert!(select.has_group_by());
        assert!(select.is_distinct());
        assert_eq!(select.get_limit(), Some(10));
        assert_eq!(select.get_skip(), Some(5));

        // Test clear methods
        select.clear_fields();
        select.clear_where_conditions();
        select.clear_order_by();
        select.clear_group_by();

        assert!(!select.has_fields());
        assert!(!select.has_where_conditions());
        assert!(!select.has_order_by());
        assert!(!select.has_group_by());
    }
}
