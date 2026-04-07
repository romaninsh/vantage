use std::fmt::{Debug, Display};

use vantage_core::util::IntoVec;
use vantage_expressions::{Expression, Expressive, expr_any};

use super::identifier::Identifier;

/// Vendor-aware string concatenation.
///
/// Renders as:
/// - **SQLite/PostgreSQL:** `a || b || c`
/// - **MySQL:**             `CONCAT(a, b, c)`
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::concat_sql;
///
/// concat_sql!(
///     ident("path").dot_of("dt"),
///     " > ",
///     ident("name").dot_of("d")
/// )
/// ```
#[derive(Debug, Clone)]
pub struct Concat<T: Debug + Display + Clone> {
    parts: Vec<Expression<T>>,
    alias: Option<String>,
}

impl<T: Debug + Display + Clone> Concat<T> {
    pub fn new(parts: impl IntoVec<Expression<T>>) -> Self {
        Self {
            parts: parts.into_vec(),
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

/// Macro to create a `Concat` from mixed `Expressive` arguments.
///
/// Each argument has `.expr()` called on it automatically, so you can
/// pass `Identifier`, `&str`, vendor expressions, etc. directly.
///
/// ```ignore
/// concat_sql!(
///     ident("path").dot_of("dt"),
///     " > ",
///     ident("name").dot_of("d")
/// )
/// ```
#[macro_export]
macro_rules! concat_sql {
    ($($part:expr),+ $(,)?) => {
        $crate::primitives::concat::Concat::new(vec![
            $({
                #[allow(unused_imports)]
                use vantage_expressions::Expressive;
                ($part).expr()
            }),+
        ])
    };
}

// -- SQLite: a || b || c -----------------------------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType>
    for Concat<crate::sqlite::types::AnySqliteType>
{
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        let base = Expression::from_vec(self.parts.clone(), " || ");
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- MySQL: CONCAT(a, b, c) --------------------------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for Concat<crate::mysql::types::AnyMysqlType> {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        let args = Expression::from_vec(self.parts.clone(), ", ");
        let base = Expression::new(
            "CONCAT({})",
            vec![vantage_expressions::ExpressiveEnum::Nested(args)],
        );
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}

// -- PostgreSQL: a || b || c --------------------------------------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType>
    for Concat<crate::postgres::types::AnyPostgresType>
{
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        let base = Expression::from_vec(self.parts.clone(), " || ");
        match &self.alias {
            Some(alias) => expr_any!("{} AS {}", (base), (Identifier::new(alias))),
            None => base,
        }
    }
}
