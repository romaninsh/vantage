//! Function-call expressions: `math::sum` and any named function.

use vantage_expressions::{Expression, Expressive};

use crate::{AnySurrealType, Expr, surreal_expr};

/// `math::sum(expr)`.
#[derive(Debug, Clone)]
pub struct Sum {
    expr: Expr,
}

impl Sum {
    pub fn new(expr: Expr) -> Self {
        Self {
            expr: surreal_expr!("math::sum({})", (expr)),
        }
    }
}

impl Expressive<AnySurrealType> for Sum {
    fn expr(&self) -> Expr {
        self.expr.clone()
    }
}

impl From<Sum> for Expr {
    fn from(val: Sum) -> Self {
        val.expr()
    }
}

/// A call `name(arg, …)` to any SurrealQL function. The name is emitted
/// verbatim, so pass a trusted built-in path such as `string::lowercase`.
#[derive(Debug, Clone)]
pub struct Fx {
    name: String,
    expr: Vec<Expr>,
}

impl Fx {
    pub fn new(name: impl Into<String>, expr: Vec<Expr>) -> Self {
        Self {
            name: name.into(),
            expr,
        }
    }
}

impl Expressive<AnySurrealType> for Fx {
    fn expr(&self) -> Expr {
        // A qualified function path isn't a column name; ⟨…⟩-escaping it
        // would break the call.
        surreal_expr!(
            "{}({})",
            (Expression::new(self.name.clone(), vec![])),
            (Expression::from_vec(self.expr.clone(), ", "))
        )
    }
}

impl From<Fx> for Expr {
    fn from(val: Fx) -> Self {
        val.expr()
    }
}
