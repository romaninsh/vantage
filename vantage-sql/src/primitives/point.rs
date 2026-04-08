use vantage_expressions::{Expression, Expressive};

/// Vendor-aware spatial point literal.
///
/// Renders as:
/// - **MySQL:**      `ST_GeomFromText('POINT(x y)', srid)`
/// - **PostgreSQL:** `ST_SetSRID(ST_MakePoint(x, y), srid)`
/// - **SQLite:**     `MakePoint(x, y, srid)` (SpatiaLite)
///
/// Default SRID is 0.
///
/// # Examples
///
/// ```ignore
/// Point::new(-0.12, 51.5)        // lon, lat for London
/// Point::new(-0.12, 51.5).srid(4326)
/// ```
#[derive(Debug, Clone)]
pub struct Point {
    x: f64,
    y: f64,
    srid: i32,
}

impl Point {
    /// Create a point with (x, y) coordinates. For geographic data, use (longitude, latitude).
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y, srid: 0 }
    }

    pub fn srid(mut self, srid: i32) -> Self {
        self.srid = srid;
        self
    }
}

// -- MySQL: ST_GeomFromText('POINT(x y)', srid) -------------------------------

#[cfg(feature = "mysql")]
impl Expressive<crate::mysql::types::AnyMysqlType> for Point {
    fn expr(&self) -> Expression<crate::mysql::types::AnyMysqlType> {
        Expression::new(
            format!(
                "ST_GeomFromText('POINT({} {})', {})",
                self.x, self.y, self.srid
            ),
            vec![],
        )
    }
}

// -- PostgreSQL: ST_SetSRID(ST_MakePoint(x, y), srid) -------------------------

#[cfg(feature = "postgres")]
impl Expressive<crate::postgres::types::AnyPostgresType> for Point {
    fn expr(&self) -> Expression<crate::postgres::types::AnyPostgresType> {
        Expression::new(
            format!(
                "ST_SetSRID(ST_MakePoint({}, {}), {})",
                self.x, self.y, self.srid
            ),
            vec![],
        )
    }
}

// -- SQLite: MakePoint(x, y, srid) (SpatiaLite) ------------------------------

#[cfg(feature = "sqlite")]
impl Expressive<crate::sqlite::types::AnySqliteType> for Point {
    fn expr(&self) -> Expression<crate::sqlite::types::AnySqliteType> {
        Expression::new(
            format!("MakePoint({}, {}, {})", self.x, self.y, self.srid),
            vec![],
        )
    }
}
