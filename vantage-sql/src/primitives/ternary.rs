use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// Vendor-aware inline conditional expression.
///
/// Renders as:
/// - **SQLite:**    `IIF(cond, true_val, false_val)`
/// - **MySQL:**     `IF(cond, true_val, false_val)`
/// - **PostgreSQL:** `CASE WHEN cond THEN true_val ELSE false_val END`
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::ternary::ternary;
///
/// // With Identifier + Operation:
/// ternary(Identifier::new("role").eq("admin"), "Yes", "No")
///     .as_alias("is_admin")
///
/// // With vendor expression for the condition:
/// ternary(
///     postgres_expr!("{} > {}", (Identifier::new("salary")), 100000.0f64),
///     "high",
///     "low",
/// )
/// ```
#[derive(Debug, Clone)]
pub struct Ternary<T: Debug + Display + Clone> {
    condition: Expression<T>,
    true_val: Expression<T>,
    false_val: Expression<T>,
}

impl<T: Debug + Display + Clone> Ternary<T> {
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

    fn args(&self) -> Vec<ExpressiveEnum<T>> {
        vec![
            ExpressiveEnum::Nested(self.condition.clone()),
            ExpressiveEnum::Nested(self.true_val.clone()),
            ExpressiveEnum::Nested(self.false_val.clone()),
        ]
    }
}

/// Shorthand for `Ternary::new(condition, true_val, false_val)`.
pub fn ternary<T: Debug + Display + Clone>(
    condition: impl Expressive<T>,
    true_val: impl Expressive<T>,
    false_val: impl Expressive<T>,
) -> Ternary<T> {
    Ternary::new(condition, true_val, false_val)
}

// -- SQLite: IIF(cond, true, false) -----------------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType>
    for Ternary<crate::sqlite::types::AnySqliteType>
{
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        Expression::new("IIF({}, {}, {})", self.args())
    }
}

// -- MySQL: IF(cond, true, false) --------------------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for Ternary<crate::mysql::types::AnyMysqlType> {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        Expression::new("IF({}, {}, {})", self.args())
    }
}

// -- PostgreSQL: CASE WHEN cond THEN true ELSE false END ---------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType>
    for Ternary<crate::postgres::types::AnyPostgresType>
{
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        Expression::new("CASE WHEN {} THEN {} ELSE {} END", self.args())
    }
}
