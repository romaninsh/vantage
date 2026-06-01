//! Generic wrapper types for the Rhai query-building DSL.

use crate::primitives::case::Case;
use crate::primitives::identifier::Identifier;
use crate::primitives::select::window::Window;
use std::fmt::{self, Debug, Display};
use vantage_expressions::Expression;

// ── RhaiIdent ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct RhaiIdent(pub Identifier);

impl RhaiIdent {
    pub fn into_inner(self) -> Identifier {
        self.0
    }
}

// ── RhaiExpr ───────────────────────────────────────────────────────────

pub struct RhaiExpr<V: Clone>(pub Expression<V>);

impl<V: Clone> Clone for RhaiExpr<V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<V: Clone + Debug + Display> Debug for RhaiExpr<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RhaiExpr").field(&self.0).finish()
    }
}

impl<V: Clone> RhaiExpr<V> {
    pub fn into_inner(self) -> Expression<V> {
        self.0
    }
}

// ── RhaiSelect ─────────────────────────────────────────────────────────

pub struct RhaiSelect<V, S, J, C> {
    pub inner: S,
    _marker: std::marker::PhantomData<(V, J, C)>,
}

impl<V, S: Clone, J, C> Clone for RhaiSelect<V, S, J, C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<V, S: Debug, J, C> Debug for RhaiSelect<V, S, J, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RhaiSelect")
            .field("inner", &self.inner)
            .finish()
    }
}

impl<V, S, J, C> RhaiSelect<V, S, J, C> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            _marker: std::marker::PhantomData,
        }
    }
    pub fn into_inner(self) -> S {
        self.inner
    }
}

// ── RhaiWindow ─────────────────────────────────────────────────────────

pub struct RhaiWindow<V: Debug + Display + Clone>(pub Window<V>);

impl<V: Debug + Display + Clone> Clone for RhaiWindow<V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<V: Debug + Display + Clone> Debug for RhaiWindow<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RhaiWindow").field(&self.0).finish()
    }
}

// ── RhaiCase ───────────────────────────────────────────────────────────

pub struct RhaiCase<V: Debug + Display + Clone>(pub Case<V>);

impl<V: Debug + Display + Clone> Clone for RhaiCase<V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<V: Debug + Display + Clone> Debug for RhaiCase<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RhaiCase").field(&self.0).finish()
    }
}

// ── Macro ──────────────────────────────────────────────────────────────

#[macro_export]
macro_rules! register_types {
    ($engine:expr, value: $V:ty, select: $Select:ty, join: $Join:ty, cond: $Cond:ty) => {{
        // Uses type aliases Sel, Id, Ex, Win, Cas from register_engine!
        $engine.register_type::<Sel>();
        $engine.register_type::<Id>();
        $engine.register_type::<Ex>();
        $engine.register_type::<Win>();
        $engine.register_type::<Cas>();
    }};
}
