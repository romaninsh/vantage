use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// Vendor-aware conditional expression.
///
/// Renders as:
/// - **SQLite:**     `IIF(cond, then, else)`
/// - **MySQL:**      `IF(cond, then, else)`
/// - **PostgreSQL:** `CASE WHEN cond THEN then ELSE else END`
///
/// # Examples
///
/// ```ignore
/// Iif::new(
///     ident("status").eq(mysql_expr!("{}", "completed")),
///     mysql_expr!("'Done'"),
///     mysql_expr!("'Active'"),
/// )
/// ```
#[derive(Debug, Clone)]
pub struct Iif<T: Debug + Display + Clone> {
    condition: Expression<T>,
    true_val: Expression<T>,
    false_val: Expression<T>,
}

impl<T: Debug + Display + Clone> Iif<T> {
    pub fn new(
        condition: impl Expressive<T>,
        true_val: impl Expressive<T>,
        false_val: impl Expressive<T>,
    ) -> Self {
        Self {
            condition: condition.expr(),
            true_val: true_val.expr(),
            false_val: false_val.expr(),
        }
    }
}

// -- SQLite: IIF(cond, then, else) --------------------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType> for Iif<crate::sqlite::types::AnySqliteType> {
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        Expression::new(
            "IIF({}, {}, {})",
            vec![
                ExpressiveEnum::Nested(self.condition.clone()),
                ExpressiveEnum::Nested(self.true_val.clone()),
                ExpressiveEnum::Nested(self.false_val.clone()),
            ],
        )
    }
}

// -- MySQL: IF(cond, then, else) ----------------------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for Iif<crate::mysql::types::AnyMysqlType> {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        Expression::new(
            "IF({}, {}, {})",
            vec![
                ExpressiveEnum::Nested(self.condition.clone()),
                ExpressiveEnum::Nested(self.true_val.clone()),
                ExpressiveEnum::Nested(self.false_val.clone()),
            ],
        )
    }
}

// -- PostgreSQL: CASE WHEN cond THEN then ELSE else END -----------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType>
    for Iif<crate::postgres::types::AnyPostgresType>
{
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        Expression::new(
            "CASE WHEN {} THEN {} ELSE {} END",
            vec![
                ExpressiveEnum::Nested(self.condition.clone()),
                ExpressiveEnum::Nested(self.true_val.clone()),
                ExpressiveEnum::Nested(self.false_val.clone()),
            ],
        )
    }
}
