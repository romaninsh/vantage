use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum, expr_any};

use super::identifier::Identifier;

/// Vendor-aware date formatting expression.
///
/// Accepts a strftime-style format string (the Rust/chrono convention) and
/// renders the appropriate function per backend:
///
/// - **SQLite:**    `STRFTIME('%Y-%m', expr)`
/// - **MySQL:**     `DATE_FORMAT(expr, '%Y-%m')`
/// - **PostgreSQL:** `TO_CHAR(expr, 'YYYY-MM')`
///
/// Supported tokens: `%Y` (4-digit year), `%m` (2-digit month), `%d` (2-digit day),
/// `%H` (24h hour), `%M` (minute), `%S` (second). Literal characters are passed through.
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::date_format::DateFormat;
///
/// DateFormat::new(ident("created_at").dot_of("o"), "%Y-%m")
///     .with_alias("month")
/// ```
#[derive(Debug, Clone)]
pub struct DateFormat<T: Debug + Display + Clone> {
    expr: Expression<T>,
    format: String,
    alias: Option<String>,
}

impl<T: Debug + Display + Clone> DateFormat<T> {
    pub fn new(expr: impl Expressive<T>, format: impl Into<String>) -> Self {
        Self {
            expr: expr.expr(),
            format: format.into(),
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

/// Shorthand for `DateFormat::new(expr, format)`.
pub fn date_format<T: Debug + Display + Clone>(
    expr: impl Expressive<T>,
    format: impl Into<String>,
) -> DateFormat<T> {
    DateFormat::new(expr, format)
}

/// Translate strftime tokens to PostgreSQL TO_CHAR tokens.
fn strftime_to_pg(fmt: &str) -> String {
    let mut out = String::with_capacity(fmt.len() * 2);
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('Y') => out.push_str("YYYY"),
                Some('m') => out.push_str("MM"),
                Some('d') => out.push_str("DD"),
                Some('H') => out.push_str("HH24"),
                Some('M') => out.push_str("MI"),
                Some('S') => out.push_str("SS"),
                Some('%') => out.push('%'),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Translate strftime tokens to MySQL DATE_FORMAT tokens.
/// MySQL is mostly the same as strftime, except minutes and seconds.
fn strftime_to_mysql(fmt: &str) -> String {
    let mut out = String::with_capacity(fmt.len());
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('M') => out.push_str("%i"),
                Some('S') => out.push_str("%s"),
                Some(other) => {
                    out.push('%');
                    out.push(other);
                }
                None => out.push('%'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// -- SQLite: STRFTIME('%Y-%m', expr) -----------------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType> for DateFormat<crate::sqlite::types::AnySqliteType> {
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        let base = Expression::new(
            "STRFTIME({}, {})",
            vec![
                ExpressiveEnum::Scalar(self.format.clone().into()),
                ExpressiveEnum::Nested(self.expr.clone()),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- MySQL: DATE_FORMAT(expr, '%Y-%m') ---------------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for DateFormat<crate::mysql::types::AnyMysqlType> {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        let fmt = strftime_to_mysql(&self.format);
        let base = Expression::new(
            "DATE_FORMAT({}, {})",
            vec![
                ExpressiveEnum::Nested(self.expr.clone()),
                ExpressiveEnum::Scalar(fmt.into()),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- PostgreSQL: TO_CHAR(expr, 'YYYY-MM') ------------------------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType> for DateFormat<crate::postgres::types::AnyPostgresType> {
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        let fmt = strftime_to_pg(&self.format);
        let base = Expression::new(
            "TO_CHAR({}, {})",
            vec![
                ExpressiveEnum::Nested(self.expr.clone()),
                ExpressiveEnum::Scalar(fmt.into()),
            ],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}
