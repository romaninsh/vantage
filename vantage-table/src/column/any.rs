use crate::column::flags::ColumnFlag;
use crate::traits::column_like::ColumnLike;
use std::collections::HashSet;
use vantage_core::*;

pub struct AnyColumn {
    inner: Box<dyn ColumnLike>,
}

impl AnyColumn {
    /// Create a new AnyColumn from any type implementing ColumnLike
    pub fn new<C: ColumnLike + 'static>(column: C) -> Self {
        Self {
            inner: Box::new(column),
        }
    }

    /// Attempt to downcast to a concrete column type
    pub fn downcast<C: ColumnLike + 'static>(self) -> Result<C> {
        self.inner
            .into_any()
            .downcast::<C>()
            .map(|boxed| *boxed)
            .map_err(|_| error!("Failed to downcast column"))
    }
}

impl ColumnLike for AnyColumn {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn alias(&self) -> Option<&str> {
        self.inner.alias()
    }

    fn flags(&self) -> HashSet<ColumnFlag> {
        self.inner.flags()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }

    fn get_type(&self) -> &'static str {
        self.inner.get_type()
    }

    fn clone_box(&self) -> Box<dyn ColumnLike> {
        Box::new(self.clone())
    }
}

impl Clone for AnyColumn {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
        }
    }
}

impl std::fmt::Debug for AnyColumn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}
