use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use vantage_core::Result;

use crate::expression::core::Expression;

pub type DeferredFuture<T> = Pin<Box<dyn Future<Output = Result<ExpressiveEnum<T>>> + Send>>;
pub type DeferredCallback<T> = Arc<dyn Fn() -> DeferredFuture<T> + Send + Sync>;

/// A deferred function that can be executed asynchronously within expressions.
///
/// `DeferredFn` enables embedding async operations, shared state reads, or database queries
/// directly into expressions without requiring async at query construction time. The deferred
/// operation is only executed when the expression is evaluated, maintaining clean separation
/// between query building and execution phases.
///
/// # Use Cases
///
/// - **Cross-database queries**: Embed API calls or database queries from other sources
/// - **Dynamic values**: Read from shared state (Arc<Mutex<T>>) at execution time
/// - **Complex operations**: Wrap expensive computations or I/O operations
/// - **Database integration**: Use `db.defer()` to create reusable query closures
///
/// # Examples
///
/// ## Using `from_fn` with async functions
///
/// ```rust
/// use vantage_expressions::{prelude::*, mocks::*};
/// use serde_json::json;
///
/// # tokio_test::block_on(async {
/// // API call that fetches data asynchronously
/// async fn measure_power_input() -> vantage_core::Result<serde_json::Value> {
///     // Simulate lightning strike measurement
///     Ok(json!(1.21)) // 1.21 jigawatts
/// }
///
/// let mock = mockbuilder::new()
///     .with_flattening()
///     .on_exact_select("SELECT is_successful FROM experiments WHERE jigawatts > 1.21", json!([
///         {"is_successful": true}
///     ]));
///
/// let query = expr!("SELECT is_successful FROM experiments WHERE jigawatts > {}",
///                  { DeferredFn::from_fn(measure_power_input) });
///
/// let result = mock.execute(&query).await.unwrap();
/// assert_eq!(result[0]["is_successful"], true);
/// # });
/// ```
///
/// ## Using `from_mutex` for shared state
///
/// ```rust
/// use std::sync::{Arc, Mutex};
/// use vantage_expressions::{prelude::*, mocks::*};
/// use serde_json::json;
///
/// # tokio_test::block_on(async {
/// // Shared plutonium level that can change over time
/// let plutonium_level = Arc::new(Mutex::new(10));
///
/// let mock = mockbuilder::new()
///     .with_flattening()
///     .on_exact_select("SELECT event FROM almanac LIMIT 1", json!([
///         {"event": "Cubs win World Series"}
///     ]))
///     .on_exact_select("SELECT event FROM almanac LIMIT 2", json!([
///         {"event": "Cubs win World Series"},
///         {"event": "Florida Gators beat Miami"}
///     ]));
///
/// // Create query with mutex-based limit
/// let deferred_limit = DeferredFn::from_mutex(plutonium_level.clone());
/// let query = expr!("SELECT event FROM almanac LIMIT {}", { deferred_limit });
///
/// // First execution with original value (10 gets clamped to 1 in our mock)
/// *plutonium_level.lock().unwrap() = 1;
/// let result1 = mock.execute(&query).await.unwrap();
/// assert_eq!(result1.as_array().unwrap().len(), 1);
///
/// // Change the limit and execute again
/// *plutonium_level.lock().unwrap() = 2;
/// let result2 = mock.execute(&query).await.unwrap();
/// assert_eq!(result2.as_array().unwrap().len(), 2);
/// # });
/// ```
///
/// ## Using `new` with custom closures - Returning Nested Expressions
///
/// This example shows how `DeferredFn` can return nested expressions instead of scalar values,
/// allowing complex query parts to be dynamically generated and embedded directly into templates:
///
/// ```rust
/// use vantage_expressions::{prelude::*, mocks::*, traits::expressive::ExpressiveEnum};
/// use serde_json::json;
///
/// # tokio_test::block_on(async {
/// let mock = mockbuilder::new()
///     .with_flattening()
///     .on_exact_select("SELECT * FROM logs WHERE destination > now()", json!([
///         {"id": 1, "destination": "1885-09-02", "traveler": "Doc Brown"}
///     ]))
///     .on_exact_select("SELECT * FROM logs WHERE destination > \"1955-11-05\"", json!([
///         {"id": 2, "destination": "2015-10-21", "traveler": "Marty McFly"}
///     ]));
///
/// // Custom closure that accepts Option<DateTime> and returns appropriate expression
/// let create_time_function = |date_override: Option<&str>| {
///     let date_str = date_override.map(|s| s.to_string());
///     DeferredFn::new(move || {
///         let date_str = date_str.clone();
///         Box::pin(async move {
///             match date_str {
///                 Some(date) => {
///                     // Return scalar date for explicit time travel destination
///                     Ok(ExpressiveEnum::Scalar(json!(date)))
///                 }
///                 None => {
///                     // Return nested expression for current time
///                     let time_expr = expr!("now()");
///                     Ok(ExpressiveEnum::Nested(time_expr))
///                 }
///             }
///         })
///     })
/// };
///
/// // Test with now() function
/// let query_now = expr!("SELECT * FROM logs WHERE destination > {}",
///                      { create_time_function(None) });
/// let result_now = mock.execute(&query_now).await.unwrap();
/// assert_eq!(result_now.as_array().unwrap().len(), 1);
///
/// // Test with explicit date (Doc Brown's famous time travel destination)
/// let query_date = expr!("SELECT * FROM logs WHERE destination > {}",
///                       { create_time_function(Some("1955-11-05")) });
/// let result_date = mock.execute(&query_date).await.unwrap();
/// assert_eq!(result_date.as_array().unwrap().len(), 1);
/// # });
/// ```
///
/// ## Cross-Database `defer()` pattern
///
/// This example demonstrates using `db.defer()` for cross-database queries where one
/// database query depends on results from another database. The first database provides
/// reference data that the second database uses for its operations:
///
/// ```rust
/// use vantage_expressions::{prelude::*, mocks::*};
/// use serde_json::json;
///
/// # tokio_test::block_on(async {
/// // Reference database - contains metadata and configuration data
/// let letter_db = mockbuilder::new()
///     .with_flattening()
///     .on_exact_select("SELECT date FROM letter WHERE recipient = \"Marty\"", json!("1885-09-02"));
///
/// // Main application database - executes operations using reference data
/// let flux_capacitor_db = mockbuilder::new()
///     .with_flattening()
///     .on_exact_select("CALL initiate_time_travel(\"1885-09-02\")", json!("Back to 1885!"));
///
/// // Create deferred query using db.defer() for cross-database operation
/// let reference_query = expr!("SELECT date FROM letter WHERE recipient = {}", "Marty");
/// let deferred_reference = letter_db.defer(reference_query);
///
/// // Use deferred query result in main database operation
/// let main_query = expr!("CALL initiate_time_travel({})", { deferred_reference });
///
/// // Execute - reference database query happens first, then main operation executes
/// let travel_result = flux_capacitor_db.execute(&main_query).await.unwrap();
/// assert_eq!(travel_result, "Back to 1885!");
/// # });
/// ```
#[derive(Clone)]
pub struct DeferredFn<T> {
    func: DeferredCallback<T>,
}

impl<T> DeferredFn<T> {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn() -> DeferredFuture<T> + Send + Sync + 'static,
    {
        Self { func: Arc::new(f) }
    }

    pub async fn call(&self) -> Result<ExpressiveEnum<T>> {
        (self.func)().await
    }

    /// Create a DeferredFn that reads from an `Arc<Mutex<T>>` when executed
    pub fn from_mutex<U>(mutex: Arc<Mutex<U>>) -> Self
    where
        U: Clone + Into<T> + Send + 'static,
        T: Send + 'static,
    {
        Self::new(move || {
            let mutex = mutex.clone();
            Box::pin(async move {
                let value = mutex.lock().unwrap().clone();
                Ok(ExpressiveEnum::Scalar(value.into()))
            })
        })
    }

    /// Create a DeferredFn from an async function, hiding the Pin logic
    pub fn from_fn<F, Fut, U>(f: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<U>> + Send + 'static,
        U: Into<T> + Send + 'static,
        T: Send + 'static,
    {
        let f = Arc::new(f);
        Self::new(move || {
            let f = f.clone();
            Box::pin(async move {
                match f().await {
                    Ok(result) => Ok(ExpressiveEnum::Scalar(result.into())),
                    Err(e) => Err(e),
                }
            })
        })
    }
}

impl<T: Debug + std::fmt::Display> Debug for DeferredFn<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_tuple("DeferredFn").field(&"<closure>").finish()
    }
}

pub enum ExpressiveEnum<T> {
    Scalar(T),
    Nested(Expression<T>),
    Deferred(DeferredFn<T>),
}

impl<T: Debug + std::fmt::Display> Debug for ExpressiveEnum<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            ExpressiveEnum::Scalar(val) => f.debug_tuple("Scalar").field(val).finish(),
            ExpressiveEnum::Nested(val) => f.debug_tuple("Nested").field(val).finish(),
            ExpressiveEnum::Deferred(deferred) => {
                f.debug_tuple("Deferred").field(deferred).finish()
            }
        }
    }
}

/// Trait for creating custom SQL constructs that can be converted to expressions.
///
/// The `Expressive` trait allows you to define reusable SQL patterns and complex
/// query constructs that can be seamlessly integrated into larger expressions.
/// This is particularly useful for creating database-specific syntax, common
/// query patterns, or complex operations like identifiers with automatic escaping.
///
/// # Example
///
/// Create a wrapper for table/field names which will escape only if we
/// use a reserved keyword.
///
/// ```rust
/// use vantage_expressions::{Expression, expr};
/// use vantage_expressions::traits::expressive::Expressive;
///
/// #[derive(Debug, Clone)]
/// pub struct Identifier {
///     identifier: String,
/// }
///
/// impl Identifier {
///     pub fn new(identifier: impl Into<String>) -> Self {
///         Self { identifier: identifier.into() }
///     }
///
///     fn needs_escaping(&self) -> bool {
///         let reserved_keywords = ["SELECT", "FROM", "TO", "IN"];
///         let upper = self.identifier.to_uppercase();
///         self.identifier.contains(' ') || reserved_keywords.contains(&upper.as_str())
///     }
/// }
///
/// impl Expressive<serde_json::Value> for Identifier {
///     fn expr(&self) -> Expression<serde_json::Value> {
///         if self.needs_escaping() {
///             expr!(format!("`{}`", self.identifier))
///         } else {
///             expr!(self.identifier.clone())
///         }
///     }
/// }
///
/// // Usage - Expressive types work automatically in expr! macro
/// let field = Identifier::new("user_name");
/// let escaped = Identifier::new("SELECT"); // Reserved keyword
///
/// // Direct usage in expr! macro - no .expr() calls needed
/// let query = expr!(
///     "SELECT {}, {}, {} FROM {}",
///     (Identifier::new("from")),
///     (Identifier::new("to")),
///     (Identifier::new("subject")),
///     (Identifier::new("emails"))
/// );
/// # assert_eq!(query.preview(), "SELECT `from`, `to`, subject FROM emails");
/// // Result: SELECT `from`, `to`, subject FROM emails
/// ```
pub trait Expressive<T> {
    /// Convert this construct into an [`Expression<T>`].
    ///
    /// This method should return an [`Expression`] that represents the SQL
    /// or query language construct. The expression can contain nested
    /// expressions, parameters, or deferred computations.
    ///
    /// Types implementing this trait can be used directly in the `expr!` macro
    /// with parentheses syntax: `(identifier)` - the conversion happens automatically.
    fn expr(&self) -> Expression<T>;

    /// Preview the expression as a formatted string.
    ///
    /// This method provides a convenient way to preview expressions without
    /// needing to call `expr().preview()` explicitly.
    fn preview(&self) -> String
    where
        T: std::fmt::Debug + std::fmt::Display,
    {
        self.expr().preview()
    }
}

impl<T: Clone> Clone for ExpressiveEnum<T> {
    fn clone(&self) -> Self {
        match self {
            ExpressiveEnum::Scalar(val) => ExpressiveEnum::Scalar(val.clone()),
            ExpressiveEnum::Nested(expr) => ExpressiveEnum::Nested(expr.clone()),
            ExpressiveEnum::Deferred(f) => ExpressiveEnum::Deferred(f.clone()),
        }
    }
}

impl<T> ExpressiveEnum<T> {
    pub fn nested(value: Expression<T>) -> Self {
        ExpressiveEnum::Nested(value)
    }

    pub fn deferred<F>(f: F) -> Self
    where
        F: Fn() -> DeferredFuture<T> + Send + Sync + 'static,
    {
        ExpressiveEnum::Deferred(DeferredFn::new(f))
    }
}

impl<T: std::fmt::Debug + std::fmt::Display> ExpressiveEnum<T> {
    pub fn preview(&self) -> String {
        match self {
            ExpressiveEnum::Scalar(val) => format!("{}", val),
            ExpressiveEnum::Nested(expr) => format!("{:?}", expr),
            ExpressiveEnum::Deferred(_) => "**deferred()".to_string(),
        }
    }
}

// Enable conversion from DeferredFn to ExpressiveEnum
impl<T> From<DeferredFn<T>> for ExpressiveEnum<T> {
    fn from(deferred: DeferredFn<T>) -> Self {
        ExpressiveEnum::Deferred(deferred)
    }
}

// Enable conversion from closures to ExpressiveEnum::Deferred
impl<T, F> From<F> for ExpressiveEnum<T>
where
    F: Fn() -> DeferredFuture<T> + Send + Sync + 'static,
{
    fn from(closure: F) -> Self {
        ExpressiveEnum::Deferred(DeferredFn::new(closure))
    }
}

// Enable conversion from serde_json::Value to ExpressiveEnum<serde_json::Value>
impl From<serde_json::Value> for ExpressiveEnum<serde_json::Value> {
    fn from(value: serde_json::Value) -> Self {
        ExpressiveEnum::Scalar(value)
    }
}

#[cfg(test)]
mod tests {}
