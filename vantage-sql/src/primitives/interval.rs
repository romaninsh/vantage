use vantage_expressions::{Expression, Expressive};

/// Vendor-aware time interval literal.
///
/// Renders as:
/// - **PostgreSQL:** `INTERVAL '1 year'`
/// - **MySQL:**      `INTERVAL 1 YEAR`
/// - **SQLite:**     `1` (numeric days — caller must convert units)
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::interval::Interval;
///
/// Interval::new(1, "year")
/// Interval::new(6, "month")
/// Interval::new(30, "day")
/// ```
#[derive(Debug, Clone)]
pub struct Interval {
    amount: i64,
    unit: String,
}

impl Interval {
    pub fn new(amount: i64, unit: impl Into<String>) -> Self {
        Self {
            amount,
            unit: unit.into().to_lowercase(),
        }
    }

    /// Convert to approximate days for SQLite (which has no INTERVAL type).
    fn to_days(&self) -> i64 {
        match self.unit.as_str() {
            "year" | "years" => self.amount * 365,
            "month" | "months" => self.amount * 30,
            "week" | "weeks" => self.amount * 7,
            "day" | "days" => self.amount,
            "hour" | "hours" => (self.amount as f64 / 24.0).round() as i64,
            _ => self.amount,
        }
    }
}

// -- SQLite: numeric days (approximate) --------------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType> for Interval {
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        Expression::new(format!("{}", self.to_days()), vec![])
    }
}

// -- MySQL: INTERVAL 1 YEAR -------------------------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for Interval {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        Expression::new(
            format!("INTERVAL {} {}", self.amount, self.unit.to_uppercase()),
            vec![],
        )
    }
}

// -- PostgreSQL: INTERVAL '1 year' ------------------------------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType> for Interval {
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        Expression::new(format!("INTERVAL '{} {}'", self.amount, self.unit), vec![])
    }
}
