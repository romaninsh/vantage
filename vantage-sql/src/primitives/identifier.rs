use vantage_expressions::{Expression, Expressive};

/// SQL identifier with proper double-quote escaping and optional alias.
///
/// Handles single identifiers (`"name"`), qualified names (`"u"."name"`),
/// and aliased expressions (`"name" AS "alias"`).
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::identifier::Identifier;
///
/// // Simple column
/// let id = Identifier::new("name");            // "name"
///
/// // Qualified (table.column)
/// let id = Identifier::with_dot("u", "name");  // "u"."name"
///
/// // With alias
/// let id = Identifier::with_dot("d", "name")
///     .with_alias("department_name");           // "d"."name" AS "department_name"
/// ```
#[derive(Debug, Clone)]
pub struct Identifier {
    parts: Vec<String>,
    alias: Option<String>,
}

impl Identifier {
    /// Single identifier: renders as `"name"`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            parts: vec![name.into()],
            alias: None,
        }
    }

    /// Qualified identifier: renders as `"prefix"."name"`.
    pub fn with_dot(prefix: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            parts: vec![prefix.into(), name.into()],
            alias: None,
        }
    }

    /// Adds an AS alias: `... AS "alias"`.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    fn render_parts(&self) -> String {
        self.parts
            .iter()
            .map(|p| format!("\"{}\"", p))
            .collect::<Vec<_>>()
            .join(".")
    }
}

impl<T> Expressive<T> for Identifier {
    fn expr(&self) -> Expression<T> {
        let sql = match &self.alias {
            Some(alias) => format!("{} AS \"{}\"", self.render_parts(), alias),
            None => self.render_parts(),
        };
        Expression::new(sql, vec![])
    }
}
