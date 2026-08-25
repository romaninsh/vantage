use vantage_expressions::{Expression, Expressive};

/// SQL identifier with optional qualification and alias.
///
/// Quoting is determined by the `Expressive<T>` impl — each backend
/// renders with its own quote style (`"` for PostgreSQL/SQLite,
/// `` ` `` for MySQL). This means `Identifier` is quote-agnostic;
/// the quoting happens only when `.expr()` is called for a specific type.
///
/// Embedded quote characters ARE escaped (doubled) when rendering, so an
/// identifier built from a runtime value cannot break out of its quotes.
/// It is still a NAME, not a value: prefer binding values as parameters
/// (`expr("… = {}", [v])`) and reserve identifiers for schema elements.
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::identifier::ident;
///
/// // Simple column — quoting depends on which Expressive<T> is used
/// let expr = mysql_expr!("SELECT {} FROM {}", (ident("name")), (ident("product")));
///
/// // Qualified (table.column)
/// let expr = mysql_expr!("SELECT {}", (ident("name").dot_of("u")));
///
/// // With alias
/// let expr = mysql_expr!("SELECT {}", (ident("name").with_alias("n")));
/// ```
#[derive(Debug, Clone)]
pub struct Identifier {
    parts: Vec<String>,
    alias: Option<String>,
}

impl Identifier {
    /// Single identifier: `name`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            parts: vec![name.into()],
            alias: None,
        }
    }

    /// Prepends a qualifier: `ident("name").dot_of("u")` → `u.name`.
    /// Chaining adds further left: `ident("col").dot_of("t").dot_of("s")` → `s.t.col`.
    pub fn dot_of(mut self, prefix: impl Into<String>) -> Self {
        self.parts.insert(0, prefix.into());
        self
    }

    /// Adds an AS alias.
    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// Returns the identifier name (parts joined with dots, no quotes).
    pub fn name(&self) -> String {
        self.parts.join(".")
    }

    /// Returns the alias, if any.
    pub fn alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    /// Render with a given quote character. Used by backend `Expressive` impls.
    ///
    /// Every quote character inside a part is DOUBLED, the escape all three
    /// supported dialects use (`"a""b"`, `` `a``b` ``). Identifiers were
    /// historically code-defined, so an unescaped `format!` was safe by
    /// construction; that stopped being true once vista scripts could read
    /// runtime values (an observation's `args`), where `ident(args.col)` on
    /// a value containing a quote would otherwise terminate the identifier
    /// and let the rest of the value be parsed as SQL.
    fn render_with(&self, q: char) -> String {
        let quote = |p: &str| format!("{q}{}{q}", p.replace(q, &format!("{q}{q}")));
        let base = self
            .parts
            .iter()
            .map(|p| quote(p))
            .collect::<Vec<_>>()
            .join(".");
        match &self.alias {
            Some(alias) => format!("{base} AS {}", quote(alias)),
            None => base,
        }
    }
}

/// Shorthand for `Identifier::new(name)`.
pub fn ident(name: impl Into<String>) -> Identifier {
    Identifier::new(name)
}

// Each backend impl owns its quoting style.

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType> for Identifier {
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        Expression::new(self.render_with('"'), vec![])
    }
}

#[cfg(feature = "sqlite")]
impl From<Identifier> for Expression<crate::sqlite::types::AnySqliteType> {
    fn from(id: Identifier) -> Self {
        id.expr()
    }
}

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType> for Identifier {
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        Expression::new(self.render_with('"'), vec![])
    }
}

#[cfg(feature = "postgres")]
impl From<Identifier> for Expression<crate::postgres::types::AnyPostgresType> {
    fn from(id: Identifier) -> Self {
        id.expr()
    }
}

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for Identifier {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        Expression::new(self.render_with('`'), vec![])
    }
}

#[cfg(feature = "mysql")]
impl From<Identifier> for Expression<crate::mysql::types::AnyMysqlType> {
    fn from(id: Identifier) -> Self {
        id.expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_and_alias_quote_normally() {
        let id = ident("name").dot_of("u").with_alias("n");
        assert_eq!(id.render_with('"'), r#""u"."name" AS "n""#);
    }

    #[test]
    fn embedded_quotes_are_doubled_not_escaped_out_of() {
        // The break-out attempt: a value that would close the identifier and
        // continue as SQL. Doubling keeps it one (absurd but inert) name.
        let id = ident(r#"x" ; DROP TABLE users --"#);
        assert_eq!(id.render_with('"'), r#""x"" ; DROP TABLE users --""#);
        let id = ident("a`b");
        assert_eq!(id.render_with('`'), "`a``b`");
    }

    #[test]
    fn alias_is_escaped_too() {
        let id = ident("col").with_alias(r#"a"b"#);
        assert_eq!(id.render_with('"'), r#""col" AS "a""b""#);
    }
}
