use std::fmt::{Debug, Display};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// Cross-database GROUP_CONCAT / STRING_AGG / array_agg aggregation.
///
/// Aggregates values from multiple rows into a single string.
#[derive(Debug, Clone)]
pub struct GroupConcat<T: Debug + Display + Clone> {
    expr: Expression<T>,
    separator: String,
    distinct: bool,
}

impl<T: Debug + Display + Clone> GroupConcat<T> {
    pub fn new(expr: impl Expressive<T>) -> Self {
        Self {
            expr: expr.expr(),
            separator: ",".to_string(),
            distinct: false,
        }
    }

    pub fn separator(mut self, sep: impl Into<String>) -> Self {
        self.separator = sep.into();
        self
    }

    pub fn distinct(mut self) -> Self {
        self.distinct = true;
        self
    }
}

// SQLite: GROUP_CONCAT(DISTINCT expr, separator)
#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType>
    for GroupConcat<crate::sqlite::types::AnySqliteType>
{
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        let distinct_kw = if self.distinct { "DISTINCT " } else { "" };
        let template = format!("GROUP_CONCAT({}{{}}, '{}')", distinct_kw, self.separator);
        Expression::new(&template, vec![ExpressiveEnum::Nested(self.expr.clone())])
    }
}

// PostgreSQL: STRING_AGG(DISTINCT expr, separator)
#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType>
    for GroupConcat<crate::postgres::types::AnyPostgresType>
{
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        let distinct_kw = if self.distinct { "DISTINCT " } else { "" };
        let template = format!("STRING_AGG({}{{}}, '{}')", distinct_kw, self.separator);
        Expression::new(&template, vec![ExpressiveEnum::Nested(self.expr.clone())])
    }
}

// MySQL: GROUP_CONCAT(DISTINCT expr SEPARATOR separator)
#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType>
    for GroupConcat<crate::mysql::types::AnyMysqlType>
{
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        let distinct_kw = if self.distinct { "DISTINCT " } else { "" };
        let template = format!(
            "GROUP_CONCAT({}{{}} SEPARATOR '{}')",
            distinct_kw, self.separator
        );
        Expression::new(&template, vec![ExpressiveEnum::Nested(self.expr.clone())])
    }
}
