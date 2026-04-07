use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use super::identifier::Identifier;

/// Vendor-aware JSON field extraction.
///
/// Renders as:
/// - **SQLite:**    `JSON_EXTRACT("col", '$.field')`
/// - **MySQL:**     `JSON_EXTRACT(\`col\`, '$.field')`
/// - **PostgreSQL:** `"col"->>'field'`
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::json_extract::JsonExtract;
///
/// // Extract a text field from a JSON column:
/// JsonExtract::new(Identifier::new("metadata"), "color")
///     .with_alias("color")
/// ```
#[derive(Debug, Clone)]
pub struct JsonExtract<T: Debug + Display + Clone> {
    source: Expression<T>,
    field: String,
    alias: Option<String>,
}

impl<T: Debug + Display + Clone> JsonExtract<T> {
    pub fn new(source: impl Expressive<T>, field: impl Into<String>) -> Self {
        Self {
            source: source.expr(),
            field: field.into(),
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

/// Shorthand for `JsonExtract::new(source, field)`.
pub fn json_extract<T: Debug + Display + Clone>(
    source: impl Expressive<T>,
    field: impl Into<String>,
) -> JsonExtract<T> {
    JsonExtract::new(source, field)
}

/// Helper: create a SQL path literal as an inline expression (not a bind parameter).
fn json_path<T: Debug + Display + Clone>(field: &str, prefix: &str) -> Expression<T> {
    Expression::new(format!("'{prefix}{field}'"), vec![])
}

// -- SQLite: JSON_EXTRACT("col", '$.field') ----------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType>
    for JsonExtract<crate::sqlite::types::AnySqliteType>
{
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        let base = Expression::new(
            "JSON_EXTRACT({}, {})",
            vec![
                ExpressiveEnum::Nested(self.source.clone()),
                ExpressiveEnum::Nested(json_path(&self.field, "$.")),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- MySQL: JSON_EXTRACT(`col`, '$.field') -----------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType>
    for JsonExtract<crate::mysql::types::AnyMysqlType>
{
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        let base = Expression::new(
            "JSON_EXTRACT({}, {})",
            vec![
                ExpressiveEnum::Nested(self.source.clone()),
                ExpressiveEnum::Nested(json_path(&self.field, "$.")),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- PostgreSQL: "col"->>'field' ---------------------------------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType>
    for JsonExtract<crate::postgres::types::AnyPostgresType>
{
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        let base = Expression::new(
            "{} ->> {}",
            vec![
                ExpressiveEnum::Nested(self.source.clone()),
                ExpressiveEnum::Nested(json_path(&self.field, "")),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}
