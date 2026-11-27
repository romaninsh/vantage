use std::fmt::{Debug, Display};

use vantage_core::util::IntoVec;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// Vendor-aware JSON field extraction.
///
/// Accepts a single field or a path of fields. For multi-level paths,
/// intermediate steps use the JSON-object accessor and the final step
/// extracts as text.
///
/// Renders as (extract as text by default):
/// - **SQLite:**     `JSON_EXTRACT("col", '$.field')`, a key that isn't an
///   identifier quoted (`'$."sr@latin"'`), as on MySQL
/// - **MySQL:**      `` JSON_UNQUOTE(JSON_EXTRACT(`col`, '$.field')) ``
/// - **PostgreSQL:** `"col" ->> 'field'` or `"col" -> 'a' ->> 'b'`
///
/// With `.as_json()` — return raw JSON instead of text:
/// - **MySQL:**      `` JSON_EXTRACT(`col`, '$.field') ``
/// - **PostgreSQL:** `"col" -> 'field'`
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
    /// When true, return the raw JSON value instead of extracting as text.
    /// Affects MySQL (drops `JSON_UNQUOTE`) and PostgreSQL (all `->` vs final `->>`)
    as_json: bool,
}

impl<T: Debug + Display + Clone> JsonExtract<T> {
    pub fn new(source: impl Expressive<T>, path: impl IntoVec<String>) -> Self {
        Self {
            source: source.expr(),
            path: path.into_vec(),
            as_json: false,
        }
    }

    /// Return raw JSON instead of text (no `JSON_UNQUOTE` in MySQL, `->` in PG).
    pub fn as_json(mut self) -> Self {
        self.as_json = true;
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

/// A JSON key that a script or a `nested_path` may name: letters, digits,
/// `_`, `-` and `@` (`en_US`, `en-US`, `sr@latin`). Keys go into the SQL as
/// literals, so a quote, backslash, `.` or `{}` never reaches the statement.
#[cfg(any(feature = "rhai", feature = "vista"))]
pub(crate) fn valid_json_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '@'))
}

/// Helper: create an inline SQL literal (not a bind parameter).
fn sql_lit<T: Debug + Display + Clone>(s: &str) -> Expression<T> {
    let escaped = s.replace('\'', "''");
    Expression::new(format!("'{escaped}'"), vec![])
}

/// A `$.a.b` JSON path, each key bare when it's an identifier and quoted
/// otherwise (`$."en-US"`, `$."sr@latin"`), as SQLite and MySQL read it.
#[cfg(any(feature = "sqlite", feature = "mysql"))]
fn dollar_path(path: &[String]) -> String {
    let key = |k: &String| {
        let mut chars = k.chars();
        let bare = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if bare {
            k.clone()
        } else {
            format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))
        }
    };
    format!("$.{}", path.iter().map(key).collect::<Vec<_>>().join("."))
}

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType>
    for JsonExtract<crate::sqlite::types::AnySqliteType>
{
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        let json_path = dollar_path(&self.path);
        Expression::new(
            "JSON_EXTRACT({}, {})",
            vec![
                ExpressiveEnum::Nested(self.source.clone()),
                ExpressiveEnum::Nested(sql_lit(&json_path)),
            ],
        )
    }
}

// The function forms of MySQL's `->>` / `->`; MariaDB has only the functions.

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType>
    for JsonExtract<crate::mysql::types::AnyMysqlType>
{
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        // MySQL takes a bare key only when it's an identifier; `en-US` or
        // `1st` must be quoted (`$."en-US"`).
        let json_path = dollar_path(&self.path);
        let template = if self.as_json {
            "JSON_EXTRACT({}, {})"
        } else {
            "JSON_UNQUOTE(JSON_EXTRACT({}, {}))"
        };
        Expression::new(
            template,
            vec![
                ExpressiveEnum::Nested(self.source.clone()),
                ExpressiveEnum::Nested(sql_lit(&json_path)),
            ],
        )
    }
}

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
            let op = if i == last && !self.as_json {
                " ->> "
            } else {
                " -> "
            };
            current = Expression::new(
                format!("{{}}{op}{{}}"),
                vec![
                    ExpressiveEnum::Nested(current),
                    ExpressiveEnum::Nested(sql_lit(field)),
                ],
            );
        }

        current
    }
}
