use std::fmt::{Debug, Display};

use vantage_core::util::IntoVec;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use super::identifier::Identifier;

/// Vendor-aware JSON field extraction.
///
/// Accepts a single field or a path of fields. For multi-level paths,
/// intermediate steps use the JSON-object accessor and the final step
/// extracts as text.
///
/// Renders as:
/// - **SQLite:**    `JSON_EXTRACT("col", '$.field')` or `JSON_EXTRACT("col", '$.a.b')`
/// - **MySQL:**     `JSON_EXTRACT(\`col\`, '$.field')` or `JSON_EXTRACT(\`col\`, '$.a.b')`
/// - **PostgreSQL:** `"col"->>'field'` or `"col"->'a'->>'b'`
///
/// # Examples
///
/// ```ignore
/// // Single field
/// JsonExtract::new(ident("metadata"), "color")
///
/// // Nested path
/// JsonExtract::new(ident("metadata"), ["specs", "voltage"])
/// ```
#[derive(Debug, Clone)]
pub struct JsonExtract<T: Debug + Display + Clone> {
    source: Expression<T>,
    path: Vec<String>,
    alias: Option<String>,
}

impl<T: Debug + Display + Clone> JsonExtract<T> {
    pub fn new(source: impl Expressive<T>, path: impl IntoVec<String>) -> Self {
        Self {
            source: source.expr(),
            path: path.into_vec(),
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

/// Shorthand for `JsonExtract::new(source, path)`.
pub fn json_extract<T: Debug + Display + Clone>(
    source: impl Expressive<T>,
    path: impl IntoVec<String>,
) -> JsonExtract<T> {
    JsonExtract::new(source, path)
}

/// Helper: create an inline SQL literal (not a bind parameter).
fn sql_lit<T: Debug + Display + Clone>(s: &str) -> Expression<T> {
    let escaped = s.replace('\'', "''");
    Expression::new(format!("'{escaped}'"), vec![])
}

// -- SQLite: JSON_EXTRACT("col", '$.a.b') ------------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType>
    for JsonExtract<crate::sqlite::types::AnySqliteType>
{
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        let json_path = format!("$.{}", self.path.join("."));
        let base = Expression::new(
            "JSON_EXTRACT({}, {})",
            vec![
                ExpressiveEnum::Nested(self.source.clone()),
                ExpressiveEnum::Nested(sql_lit(&json_path)),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- MySQL: JSON_EXTRACT(`col`, '$.a.b') -------------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType>
    for JsonExtract<crate::mysql::types::AnyMysqlType>
{
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        let json_path = format!("$.{}", self.path.join("."));
        let base = Expression::new(
            "JSON_EXTRACT({}, {})",
            vec![
                ExpressiveEnum::Nested(self.source.clone()),
                ExpressiveEnum::Nested(sql_lit(&json_path)),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- PostgreSQL: "col"->'a'->>'b' --------------------------------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType>
    for JsonExtract<crate::postgres::types::AnyPostgresType>
{
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        // Build chain: source -> 'a' -> 'b' ->> 'last'
        // All intermediate steps use -> (returns jsonb), final step uses ->> (returns text)
        assert!(
            !self.path.is_empty(),
            "JsonExtract requires at least one path segment"
        );
        let mut current = self.source.clone();
        let last = self.path.len() - 1;

        for (i, field) in self.path.iter().enumerate() {
            let op = if i == last { " ->> " } else { " -> " };
            current = Expression::new(
                format!("{{}}{op}{{}}"),
                vec![
                    ExpressiveEnum::Nested(current),
                    ExpressiveEnum::Nested(sql_lit(field)),
                ],
            );
        }

        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (current), (Identifier::new(alias))),
            None => current,
        }
    }
}
