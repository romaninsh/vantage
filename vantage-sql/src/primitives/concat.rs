use std::fmt::{Debug, Display};

use vantage_core::util::IntoVec;
use vantage_expressions::{Expression, Expressive};

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
    separator: Option<Expression<T>>,
}

impl<T: Debug + Display + Clone> Concat<T> {
    pub fn new(parts: impl IntoVec<Expression<T>>) -> Self {
        Self {
            parts: parts.into_vec(),
            separator: None,
        }
    }

    /// Use CONCAT_WS with a separator instead of plain CONCAT.
    pub fn ws(mut self, separator: impl Expressive<T>) -> Self {
        self.separator = Some(separator.expr());
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

// -- SQLite: a || b || c or a || sep || b || sep || c -------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType>
    for Concat<crate::sqlite::types::AnySqliteType>
{
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        if let Some(sep) = &self.separator {
            // Interleave parts with separator: a || sep || b || sep || c
            let mut interleaved = Vec::with_capacity(self.parts.len() * 2 - 1);
            for (i, part) in self.parts.iter().enumerate() {
                if i > 0 {
                    interleaved.push(sep.clone());
                }
                interleaved.push(part.clone());
            }
            Expression::from_vec(interleaved, " || ")
        } else {
            Expression::from_vec(self.parts.clone(), " || ")
        }
    }
}

// -- MySQL: CONCAT(a, b, c) or CONCAT_WS(sep, a, b, c) ----------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for Concat<crate::mysql::types::AnyMysqlType> {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        use vantage_expressions::ExpressiveEnum;

        if let Some(sep) = &self.separator {
            let mut all = vec![sep.clone()];
            all.extend(self.parts.clone());
            let args = Expression::from_vec(all, ", ");
            Expression::new("CONCAT_WS({})", vec![ExpressiveEnum::Nested(args)])
        } else {
            let args = Expression::from_vec(self.parts.clone(), ", ");
            Expression::new("CONCAT({})", vec![ExpressiveEnum::Nested(args)])
        }
    }
}

// -- PostgreSQL: a || b || c or CONCAT_WS(sep, a, b, c) ----------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType>
    for Concat<crate::postgres::types::AnyPostgresType>
{
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        use vantage_expressions::ExpressiveEnum;

        if let Some(sep) = &self.separator {
            let mut all = vec![sep.clone()];
            all.extend(self.parts.clone());
            let args = Expression::from_vec(all, ", ");
            Expression::new("CONCAT_WS({})", vec![ExpressiveEnum::Nested(args)])
        } else {
            Expression::from_vec(self.parts.clone(), " || ")
        }
    }
}
