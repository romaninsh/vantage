//! # SurrealDB Select Query Builder
//!
//! Builds SELECT query for SurrealDB. Implements [`Selectable`] protocol.

// pub mod expressive;
pub mod field;
pub mod select_field;
pub mod target;

use std::marker::PhantomData;

use crate::AnySurrealType;

use field::Field;
use select_field::SelectField;
use target::Target;
use vantage_core::Result;

use crate::{
    Expr,
    identifier::Identifier,
    sum::{Fx, Sum},
    surreal_expr,
    surreal_return::SurrealReturn,
    surrealdb::SurrealDB,
};
use vantage_expressions::{
    ExprDataSource, Expression, Expressive, ExpressiveEnum,
    result::{self, QueryResult},
    traits::selectable::{Selectable, SourceRef},
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
    pub where_conditions: Vec<Expr>,
    pub order_by: Vec<(Expr, bool)>,
    pub group_by: Vec<Expr>,
    pub distinct: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
    _phantom: PhantomData<T>,
}

impl SurrealSelect<result::Single> {
    pub async fn get(&self, db: &SurrealDB) -> Result<AnySurrealType> {
        db.execute(&self.expr()).await
    }
}

impl SurrealSelect<result::List> {
    pub async fn get(&self, db: &SurrealDB) -> Result<Vec<AnySurrealType>> {
        db.execute(&self.expr())
            .await?
            .try_get()
            .ok_or_else(|| vantage_core::error!("Expected array from database query"))
    }
}

impl SurrealSelect<result::Rows> {
    pub async fn get(
        &self,
        db: &SurrealDB,
    ) -> Result<Vec<indexmap::IndexMap<String, AnySurrealType>>> {
        db.execute(&self.expr())
            .await?
            .try_get()
            .ok_or_else(|| vantage_core::error!("Expected array of objects from database query"))
    }
}

impl SurrealSelect<result::SingleRow> {
    pub async fn get(&self, db: &SurrealDB) -> Result<indexmap::IndexMap<String, AnySurrealType>> {
        db.execute(&self.expr())
            .await?
            .try_get()
            .ok_or_else(|| vantage_core::error!("Expected object from database query"))
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

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.fields.push(SelectField::new(Identifier::new(field)));
        self
    }
    /// Renders the SELECT fields clause
    ///
    /// doc wip
    fn render_fields(&self) -> Expr {
        if self.fields.is_empty() {
            surreal_expr!("*")
        } else {
            let field_expressions: Vec<Expr> = self
                .fields
                .iter()
                .map(|field| field.clone().into())
                .collect();
            Expression::from_vec(field_expressions, ", ")
        }
    }

    /// Renders the FROM clause
    ///
    /// doc wip
    fn render_from(&self) -> Expr {
        if self.from.is_empty() {
            surreal_expr!("")
        } else {
            let from_expressions: Vec<Expr> = self
                .from
                .iter()
                .map(|target| target.clone().into())
                .collect();
            surreal_expr!(
                format!(" FROM {}{{}}", if self.from_only { "ONLY " } else { "" }),
                (Expression::from_vec(from_expressions, ", "))
            )
        }
    }

    /// Renders the WHERE clause
    ///
    /// doc wip
    fn render_where(&self) -> Expr {
        if self.where_conditions.is_empty() {
            surreal_expr!("")
        } else {
            // Combine multiple conditions with AND
            let combined = Expression::from_vec(self.where_conditions.clone(), " AND ");
            surreal_expr!(" WHERE {}", (combined))
        }
    }

    /// Renders the GROUP BY clause
    ///
    /// doc wip
    fn render_group_by(&self) -> Expr {
        if self.group_by.is_empty() {
            surreal_expr!("")
        } else {
            let group_expressions: Vec<Expr> = self.group_by.to_vec();
            surreal_expr!(
                " GROUP BY {}",
                (Expression::from_vec(group_expressions, ", "))
            )
        }
    }

    /// Renders the ORDER BY clause
    ///
    /// doc wip
    fn render_order_by(&self) -> Expr {
        if self.order_by.is_empty() {
            surreal_expr!("")
        } else {
            let order_expressions: Vec<Expr> = self
                .order_by
                .iter()
                .map(|(expression, ascending)| {
                    if *ascending {
                        surreal_expr!("{}", (expression.clone()))
                    } else {
                        surreal_expr!("{} DESC", (expression.clone()))
                    }
                })
                .collect();
            let combined = Expression::from_vec(order_expressions, ", ");
            surreal_expr!(" ORDER BY {}", (combined))
        }
    }

    /// Renders the LIMIT and START clauses
    ///
    /// doc wip
    fn render_limit(&self) -> Expr {
        match (self.limit, self.skip) {
            (Some(limit), Some(skip)) => surreal_expr!(" LIMIT {} START {}", limit, skip),
            (Some(limit), None) => surreal_expr!(" LIMIT {}", limit),
            (None, Some(skip)) => surreal_expr!(" START {}", skip),
            (None, None) => surreal_expr!(""),
        }
    }

    /// Renders entire statement into an expression
    fn render(&self) -> Expr {
        surreal_expr!(
            "SELECT {}{}{}{}{}{}{}",
            (if self.single_value {
                surreal_expr!("VALUE ")
            } else {
                surreal_expr!("")
            }),
            (self.render_fields()),
            (self.render_from()),
            (self.render_where()),
            (self.render_group_by()),
            (self.render_order_by()),
            (self.render_limit())
        )
    }

    /// Renders everything into a string. Use for
    /// debug only. Never or use as part of another query!!
    pub fn preview(&self) -> String {
        self.render().preview()
    }
}

impl<T: QueryResult> Expressive<AnySurrealType> for SurrealSelect<T> {
    fn expr(&self) -> Expr {
        self.render()
    }
}

// impl<T: QueryResult> Into<Expression> for SurrealSelect<T> {
//     fn into(self) -> Expression {
//         self.render()
//     }
// }

impl<T: QueryResult> Selectable<crate::AnySurrealType> for SurrealSelect<T> {
    fn set_source(
        &mut self,
        source: impl Into<SourceRef<crate::AnySurrealType>>,
        _alias: Option<String>,
    ) {
        use vantage_expressions::ExpressiveEnum;
        let source_ref = source.into();
        let source_expr = match source_ref.into_expressive_enum() {
            ExpressiveEnum::Scalar(s) => {
                let source = s
                    .try_get::<String>()
                    .unwrap_or_else(|_| panic!("Source must be a string, found {:?}", s));
                Identifier::new(source).expr()
            }
            ExpressiveEnum::Nested(expr) => surreal_expr!("({})", (expr)),
            ExpressiveEnum::Deferred(_deferred_fn) => {
                panic!("Cannot use deferred as select source")
            }
        };
        self.from = vec![Target::new(source_expr)];
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.fields.push(SelectField::new(Identifier::new(field)));
    }

    fn add_expression(&mut self, expression: Expr, alias: Option<String>) {
        let mut field = SelectField::new(expression);
        if let Some(alias) = alias {
            field = field.with_alias(alias);
        }
        self.fields.push(field);
    }

    fn add_where_condition(&mut self, condition: Expr) {
        self.where_conditions.push(condition);
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.distinct = distinct;
    }

    fn add_order_by(&mut self, expression: Expr, ascending: bool) {
        self.order_by.push((expression, ascending));
    }

    fn add_group_by(&mut self, expression: Expr) {
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

    fn as_count(&self) -> Expr {
        use crate::sum::Fx;

        // SurrealDB syntax: count(id) wrapped in function call
        let id_expr = surreal_expr!("id");
        Fx::new("count", vec![id_expr]).into()
    }

    fn as_sum(&self, column: Expr) -> Expr {
        use crate::sum::Sum;

        // SurrealDB syntax: math::sum(column)
        Sum::new(column).into()
    }
}

impl SurrealSelect<result::Rows> {
    pub fn as_sum(self, field_or_expr: impl Into<ExpressiveEnum<AnySurrealType>>) -> SurrealReturn {
        let query = self.without_fields();
        let query = match field_or_expr.into() {
            ExpressiveEnum::Scalar(s) => query.only_column(s.try_get::<String>().unwrap()),
            ExpressiveEnum::Nested(e) => query.only_expression(e),
            _ => panic!("Only scalar string or nested is acceptable"),
        };

        SurrealReturn::new(Sum::new(query.expr()).into())
    }
    pub fn as_count(self) -> SurrealReturn {
        let result = self.only_expression(surreal_expr!("id"));
        SurrealReturn::new(Fx::new("count", vec![result.expr()]).into())
    }
    pub fn only_expression(self, expr: Expr) -> SurrealSelect<result::List> {
        self.without_fields()
            .with_expression(expr, None)
            .into_list()
    }
    pub fn only_column(self, column: impl Into<String>) -> SurrealSelect<result::List> {
        self.without_fields().with_field(column).into_list()
    }
    fn into_list(self) -> SurrealSelect<result::List> {
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
impl SurrealSelect<result::List> {
    pub fn only_first_row(self) -> SurrealSelect<result::Single> {
        if self.from_only {
            panic!("SelectQuery<List>::only_first_row() must not have from_only=true");
        }
        if !self.single_value {
            panic!("SelectQuery<List>::only_first_row() must have single_value=true");
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
            single_value: self.single_value,
        }
    }
}

impl SurrealSelect<result::SingleRow> {
    pub fn only_expression(self, expr: Expr) -> SurrealSelect<result::Single> {
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

// impl crate::protocol::SurrealQueriable for SurrealSelect<T> {}

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
            .from(vec![Target::new(surreal_expr!("users"))]);

        let sql = select.preview();

        assert_eq!(sql, "SELECT name, ⟨set⟩ FROM users");
    }

    #[test]
    fn test_select_all() {
        let select = SurrealSelect::new().from(vec![Target::new(surreal_expr!("users"))]);

        let sql = select.preview();

        assert_eq!(sql, "SELECT * FROM users");
    }

    #[test]
    fn test_select_with_where_condition() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_where_condition(surreal_expr!("age > 18"));

        let sql = select.preview();

        assert_eq!(sql, "SELECT name FROM users WHERE age > 18");
    }

    #[test]
    fn test_select_with_multiple_where_conditions() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_where_condition(surreal_expr!("age > 18"));
        select.add_where_condition(surreal_expr!("active = true"));

        let sql = select.preview();

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
        select.add_order_by(surreal_expr!("name"), true);

        let sql = select.preview();

        assert_eq!(sql, "SELECT name FROM users ORDER BY name");
    }

    #[test]
    fn test_select_with_order_by_desc() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_order_by(surreal_expr!("created_at"), false);

        let sql = select.preview();

        assert_eq!(sql, "SELECT name FROM users ORDER BY created_at DESC");
    }

    #[test]
    fn test_select_with_group_by() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("department".to_string());
        select.add_expression(surreal_expr!("count()"), Some("count".to_string()));
        select.add_group_by(surreal_expr!("department"));

        let sql = select.preview();

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

        let sql = select.preview();

        assert_eq!(sql, "SELECT name FROM users LIMIT 10");
    }

    #[test]
    fn test_select_with_limit_and_start() {
        let mut select = SurrealSelect::new();
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.set_limit(Some(10), Some(20));

        let sql = select.preview();

        assert_eq!(sql, "SELECT name FROM users LIMIT 10 START 20");
    }

    #[test]
    fn test_complex_select_query() {
        let mut select = SurrealSelect::new();
        select.set_source("orders", None);
        select.add_field("customer_id".to_string());
        select.add_expression(
            surreal_expr!("SUM(total)"),
            Some("total_amount".to_string()),
        );
        select.add_where_condition(surreal_expr!("status = 'completed'"));
        select.add_group_by(surreal_expr!("customer_id"));
        select.add_order_by(surreal_expr!("total_amount"), false);
        select.set_limit(Some(5), None);

        let sql = select.preview();

        assert_eq!(
            sql,
            "SELECT customer_id, SUM(total) AS total_amount FROM orders WHERE status = 'completed' GROUP BY customer_id ORDER BY total_amount DESC LIMIT 5"
        );
    }

    #[test]
    fn test_selectable_trait_methods() {
        let mut select = SurrealSelect::new();

        // Test Selectable trait methods
        select.set_source("users", None);
        select.add_field("name".to_string());
        select.add_field("email".to_string());
        select.add_expression(surreal_expr!("age * 2"), Some("double_age".to_string()));
        select.add_where_condition(surreal_expr!("age > 18"));
        select.add_order_by(surreal_expr!("name"), true);
        select.add_group_by(surreal_expr!("department"));
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
