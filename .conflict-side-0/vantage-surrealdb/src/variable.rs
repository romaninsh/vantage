//! # SurrealDB Variables
//!
//! doc wip

use vantage_expressions::{Expression, expr};

/// SurrealDB variable representation
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_surrealdb::variable::Variable;
///
/// // doc wip
/// let var = Variable::new("user_id".to_string());
/// ```

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Variable {
    name: String,
}

impl Variable {
    /// Creates a new variable
    ///
    /// doc wip
    ///
    /// # Arguments
    ///
    /// * `name` - doc wip
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl From<Variable> for Expression {
    fn from(val: Variable) -> Self {
        expr!(format!("${}", val.name))
    }
}
