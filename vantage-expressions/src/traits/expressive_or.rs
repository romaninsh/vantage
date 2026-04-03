use crate::expression::core::Expression;

/// Like [`Expressive`](crate::Expressive), but with an alternative conversion for string types.
///
/// Both traits convert values into `Expression<T>`, but differ in how
/// they treat `&str` and `String`:
///
/// | Type     | `Expressive<T>`        | `ExpressiveOr<T, Identifier>` |
/// |----------|------------------------|-------------------------------|
/// | `&str`   | quoted string literal   | column/field identifier        |
/// | `String` | quoted string literal   | column/field identifier        |
/// | `Expr`   | pass through            | pass through                   |
/// | `Field`  | `.expr()`               | `.expr()`                      |
///
/// The `Or` type parameter determines how strings are wrapped. Each
/// datasource crate provides impls for its own `Or` type (e.g. `Identifier`).
///
/// ## Usage as method argument
///
/// ```ignore
/// // Accept column names as strings or raw expressions:
/// pub fn with_order_by(mut self, field: impl ExpressiveOr<AnySurrealType, Identifier>) -> Self {
///     self.order_by.push(field.field_expr());
///     self
/// }
///
/// // Callers can pass either:
/// select.with_order_by("name")                    // &str → Identifier → unquoted
/// select.with_order_by(surreal_expr!("a + b"))    // Expr → pass through
/// select.with_order_by(Field::new("price"))       // Field → .field_expr()
/// ```
///
/// ## Implementing for a datasource
///
/// ```ignore
/// // Strings go through Identifier (the Or type)
/// impl ExpressiveOr<AnySurrealType, Identifier> for &str {
///     fn field_expr(&self) -> Expr { Identifier::new(self).expr() }
/// }
/// impl ExpressiveOr<AnySurrealType, Identifier> for String { ... }
///
/// // Everything else passes through via Expressive
/// impl ExpressiveOr<AnySurrealType, Identifier> for Field { ... }
/// impl ExpressiveOr<AnySurrealType, Identifier> for Expr { ... }
/// impl ExpressiveOr<AnySurrealType, Identifier> for Column<T> { ... }
/// ```
///
/// No blanket impl is provided because `&str` and `String` already implement
/// `Expressive<T>` with different (quoted-literal) semantics, which would conflict.
pub trait ExpressiveOr<T, Or> {
    fn field_expr(&self) -> Expression<T>;
}
