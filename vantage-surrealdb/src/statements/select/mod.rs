//! # SurrealDB Select Query Builder
//!
//! Builds SELECT query for SurrealDB. Implements [`Selectable`] protocol.

pub mod builder;
pub mod exec;
pub mod impls;
pub mod render;
pub mod select_field;
pub mod select_target;
pub mod transform;

#[cfg(test)]
mod tests;

use std::marker::PhantomData;

use crate::Expr;
use select_field::SelectField;
use select_target::SelectTarget;
use vantage_expressions::result;

/// SurrealDB SELECT query builder
///
/// doc wip
///
/// # Examples
///
/// ```rust
/// use vantage_expressions::Selectable;
/// use vantage_surrealdb::{select::SurrealSelect, surreal_expr};
///
/// let mut select = SurrealSelect::new();
/// select.add_source("users", None);
/// select.add_field("name");
/// ```
#[derive(Debug, Clone)]
pub struct SurrealSelect<T = result::Rows> {
    pub fields: Vec<SelectField>,
    pub fields_omit: Vec<String>,
    pub(crate) single_value: bool,
    pub(crate) from_only: bool,
    pub from: Vec<SelectTarget>,
    pub from_omit: bool,
    pub where_conditions: Vec<Expr>,
    pub order_by: Vec<(Expr, bool)>,
    pub group_by: Vec<Expr>,
    pub distinct: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T> Default for SurrealSelect<T> {
    fn default() -> Self {
        Self {
            fields: Vec::new(),
            fields_omit: Vec::new(),
            single_value: false,
            from: Vec::new(),
            from_omit: false,
            from_only: false,
            where_conditions: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            distinct: false,
            limit: None,
            skip: None,
            _phantom: PhantomData,
        }
    }
}
