use std::fmt::{Debug, Display};

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

#[derive(Debug, Clone)]
enum CompoundOp {
    Union,
    UnionAll,
    Except,
    Intersect,
}

impl CompoundOp {
    fn as_str(&self) -> &'static str {
        match self {
            CompoundOp::Union => " UNION ",
            CompoundOp::UnionAll => " UNION ALL ",
            CompoundOp::Except => " EXCEPT ",
            CompoundOp::Intersect => " INTERSECT ",
        }
    }
}

/// Compound query: combines multiple SELECT statements with UNION / UNION ALL / EXCEPT / INTERSECT.
///
/// # Examples
///
/// ```ignore
/// Union::new(first_select)
///     .union_all(second_select)
///     .except(third_select)
/// ```
#[derive(Debug, Clone)]
pub struct Union<T: Debug + Display + Clone> {
    first: Expression<T>,
    rest: Vec<(CompoundOp, Expression<T>)>,
}

impl<T: Debug + Display + Clone> Union<T> {
    pub fn new(first: impl Expressive<T>) -> Self {
        Self {
            first: first.expr(),
            rest: Vec::new(),
        }
    }

    pub fn union(mut self, query: impl Expressive<T>) -> Self {
        self.rest.push((CompoundOp::Union, query.expr()));
        self
    }

    pub fn union_all(mut self, query: impl Expressive<T>) -> Self {
        self.rest.push((CompoundOp::UnionAll, query.expr()));
        self
    }

    pub fn except(mut self, query: impl Expressive<T>) -> Self {
        self.rest.push((CompoundOp::Except, query.expr()));
        self
    }

    pub fn intersect(mut self, query: impl Expressive<T>) -> Self {
        self.rest.push((CompoundOp::Intersect, query.expr()));
        self
    }
}

impl<T: Debug + Display + Clone> Expressive<T> for Union<T> {
    fn expr(&self) -> Expression<T> {
        let mut params = vec![ExpressiveEnum::Nested(self.first.clone())];
        let mut template = String::from("{}");

        for (op, query) in &self.rest {
            template.push_str(op.as_str());
            template.push_str("{}");
            params.push(ExpressiveEnum::Nested(query.clone()));
        }

        Expression::new(template, params)
    }
}
