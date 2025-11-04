use crate::Selectable;

/// Tables or other sets implement Queryable when it is possible to
/// spawn query from it.
pub trait Queryable {
    type SelectType: Selectable;
    fn select(&self) -> Self::SelectType;
}
