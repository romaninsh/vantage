//! The immutable table handle scripts narrow: a base plus a list of steps.
//!
//! Every narrowing verb returns a new [`Handle`] that shares its prefix with
//! the one it came from, so a handle stored in a variable never changes. A
//! [`Vista`] is only built when a terminal asks for one, through
//! [`Handle::resolve`].
//!
//! In a `modify:` script, `self` is a handle over the Vista being
//! changed, and it follows the same rule: `self.where(..)` returns a new
//! handle, and a statement whose result is dropped changes nothing. The
//! script's value is the handle it ends on.

mod query;
mod step;

use std::sync::Arc;

use vantage_core::{Context, Result, error};

pub(crate) use step::Step;

use crate::{NativeCondition, VistaResolver, vista::Vista};

#[derive(Clone)]
pub struct Handle {
    base: Base,
    steps: Arc<Vec<Step>>,
    /// The resolver `table(name)` was called with, used when a caller
    /// resolves without one.
    resolver: Option<Arc<dyn VistaResolver>>,
    /// The extension verb that last narrowed the base with a native
    /// condition, which [`Handle::query`] can't express.
    native_verb: Option<String>,
}

#[derive(Clone)]
enum Base {
    /// Resolved by name through a [`VistaResolver`].
    Name(String),
    /// A Vista in hand, cloned on each resolve.
    Vista(Arc<Vista>),
}

impl Handle {
    /// A handle on the table `name`, resolved when a terminal reads it.
    pub fn named(name: impl Into<String>) -> Self {
        Self::with_base(Base::Name(name.into()))
    }

    /// A handle over `vista`: the `self` of a `modify:` script.
    pub(crate) fn over(vista: Vista) -> Self {
        Self::with_base(Base::Vista(Arc::new(vista)))
    }

    fn with_base(base: Base) -> Self {
        Self {
            base,
            steps: Arc::new(Vec::new()),
            resolver: None,
            native_verb: None,
        }
    }

    /// The resolver a named base goes through when a caller resolves without
    /// one. A [`RecordDraft`](super::RecordDraft) reads and writes through it.
    pub fn with_resolver(mut self, resolver: Arc<dyn VistaResolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    /// The table name this handle starts from; `None` for a handle over a Vista.
    pub fn table_name(&self) -> Option<&str> {
        match &self.base {
            Base::Name(name) => Some(name),
            Base::Vista(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }

    /// A new handle with `step` appended; `self` is left as it was.
    pub(crate) fn push(&self, step: Step) -> Handle {
        let mut steps = Vec::with_capacity(self.steps.len() + 1);
        steps.extend(self.steps.iter().cloned());
        steps.push(step);
        Handle {
            steps: Arc::new(steps),
            ..self.clone()
        }
    }

    /// Build the Vista: the base (resolved by name, or a clone of the Vista
    /// in hand), then every step in order. A named base goes through
    /// `resolver`, else the one `table(name)` was given.
    pub fn resolve(&self, resolver: Option<&dyn VistaResolver>) -> Result<Vista> {
        let mut vista = self.resolve_base(resolver)?;
        let mut ref_cap = None;
        for step in self.steps.iter() {
            vista = step
                .apply(vista, ref_cap)
                .with_context(|| error!("Step can't be applied", step = step))?;
            ref_cap = step.ref_cap_after(ref_cap);
        }
        Ok(vista)
    }

    /// The Vista writes go to: the whole narrowed handle — `where`, `search`
    /// and `ref` all define the set, so a write only touches rows in it.
    /// `sort` doesn't change membership and is skipped. `limit` defines rows
    /// by position, not by condition, so a handle with one can't be written
    /// through.
    pub(crate) fn write_target(&self, resolver: Option<&dyn VistaResolver>) -> Result<Vista> {
        if self.steps.iter().any(|s| matches!(s, Step::Limit(_))) {
            return Err(
                error!("writes need a set defined by conditions; drop limit(n)")
                    .mark_incorrect_usage(),
            );
        }
        let mut vista = self.resolve_base(resolver)?;
        for step in self.steps.iter() {
            if matches!(step, Step::Sort { .. }) {
                continue;
            }
            vista = step
                .apply(vista, None)
                .with_context(|| error!("Step can't be applied", step = step))?;
        }
        Ok(vista)
    }

    /// The smallest `limit(n)` applied after the last `ref` step.
    pub(crate) fn row_limit(&self) -> Option<usize> {
        self.steps
            .iter()
            .rev()
            .take_while(|s| !matches!(s, Step::Ref(_)))
            .filter_map(|s| match s {
                Step::Limit(n) => Some(*n),
                _ => None,
            })
            .min()
    }

    /// A handle whose Vista base is a copy narrowed by `condition`, keeping
    /// its steps; `self` is left as it was. A backend's extension verb
    /// (`verb`, such as `with_condition`) uses this. The base must be a
    /// Vista in hand, so a handle built from `table(name)`, or one past a
    /// `ref`, is an error naming the verb.
    pub fn with_native_condition(&self, verb: &str, condition: NativeCondition) -> Result<Handle> {
        let Base::Vista(source) = &self.base else {
            return Err(error!(
                "Extension verb only works on `self`, not on a handle from `table(...)`",
                verb = verb
            ));
        };
        if self.steps.iter().any(|s| matches!(s, Step::Ref(_))) {
            return Err(error!(
                "Extension verb can't follow `ref(...)`",
                verb = verb
            ));
        }
        let narrowed = Vista::clone(source).with_native_condition(condition)?;
        Ok(Handle {
            base: Base::Vista(Arc::new(narrowed)),
            native_verb: Some(verb.to_string()),
            ..self.clone()
        })
    }

    fn resolve_base(&self, resolver: Option<&dyn VistaResolver>) -> Result<Vista> {
        match &self.base {
            Base::Name(name) => resolver
                .or(self.resolver.as_deref())
                .ok_or_else(|| {
                    error!(
                        "Table can't be resolved here: no tables are available",
                        table = name
                    )
                })?
                .vista(name),
            Base::Vista(vista) => Ok(Vista::clone(vista)),
        }
    }
}
