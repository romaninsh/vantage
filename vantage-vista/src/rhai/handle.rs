//! The immutable table handle scripts narrow: a base plus a list of steps.
//!
//! Every narrowing verb returns a new [`Handle`] that shares its prefix with
//! the one it came from, so a handle stored in a variable never changes. A
//! [`Vista`] is only built when a terminal asks for one, through
//! [`Handle::resolve`].

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, error};

use super::traverse::traverse;
use super::vocab::TargetResolver;
use crate::{FilterOp, sort::SortDirection, vista::Vista};

#[derive(Clone)]
pub struct Handle {
    base: Base,
    steps: Arc<Vec<Step>>,
    /// The resolver `table(name)` was called with, used when a caller
    /// resolves without one.
    resolver: Option<TargetResolver>,
    /// Shared by every handle derived from one [`Handle::over`]: the latest
    /// handle an extension verb produced, so a script that ends on a
    /// statement (`self.with_condition(..);`) still yields that change.
    latest: Option<Arc<Mutex<Option<Handle>>>>,
}

#[derive(Clone)]
pub(crate) enum Base {
    /// Resolved by name through the host's [`TargetResolver`].
    Name(String),
    /// A Vista in hand. Copied with `clone_shell` on each resolve; a backend
    /// that can't be copied hands its Vista over once.
    Vista(Arc<Mutex<Option<Vista>>>),
}

/// One narrowing step, as a host reading a handle's description sees it.
#[derive(Clone, Debug)]
pub enum Step {
    Where {
        col: String,
        op: FilterOp,
        value: CborValue,
    },
    Sort {
        col: String,
        dir: SortDirection,
    },
    Search(String),
    Limit(usize),
    Ref(String),
}

impl Handle {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            base: Base::Name(name.into()),
            steps: Arc::new(Vec::new()),
            resolver: None,
            latest: None,
        }
    }

    pub fn over(vista: Vista) -> Self {
        Self {
            base: Base::Vista(Arc::new(Mutex::new(Some(vista)))),
            steps: Arc::new(Vec::new()),
            resolver: None,
            latest: Some(Arc::default()),
        }
    }

    /// The latest handle an extension verb derived from this one's base, if
    /// any.
    pub(crate) fn latest_extended(&self) -> Option<Handle> {
        self.latest.as_ref()?.lock().ok()?.clone()
    }

    pub(crate) fn with_resolver(mut self, resolver: TargetResolver) -> Self {
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

    pub fn steps(&self) -> &[Step] {
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

    /// Build the Vista: resolver(name) or a clone_shell of the base, then apply
    /// steps. Without `resolver`, the one `table(name)` was given is used.
    pub fn resolve(&self, resolver: Option<&TargetResolver>) -> Result<Vista> {
        self.resolve_through(resolver, self.steps.len())
    }

    /// The Vista writes go to: the base, or the target of the last `ref` step.
    /// Other narrowing doesn't filter writes.
    pub(crate) fn write_target(&self, resolver: Option<&TargetResolver>) -> Result<Vista> {
        let through = self
            .steps
            .iter()
            .rposition(|s| matches!(s, Step::Ref(_)))
            .map_or(0, |i| i + 1);
        self.resolve_through(resolver, through)
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

    /// A handle over a changed copy of the Vista base (or of the latest
    /// extension verb's result from the same `self`), keeping the steps;
    /// `self` is left as it was. Backend extension verbs (`verb`) use this:
    /// they need a Vista in hand, so a handle built from `table(name)`, or one
    /// past a `ref`, is an error naming the verb. A backend that can't be
    /// copied is changed in place.
    pub fn with_base_vista(
        &self,
        verb: &str,
        f: impl FnOnce(&mut Vista) -> Result<()>,
    ) -> Result<Handle> {
        let Base::Vista(cell) = &self.base else {
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
        // After an earlier extension verb on `self`, build on its result, so
        // `self.a(); self.b();` applies both.
        let source = match self.latest_extended().map(|h| h.base) {
            Some(Base::Vista(latest)) => latest,
            _ => cell.clone(),
        };
        let mut slot = source
            .lock()
            .map_err(|_| error!("table handle mutex poisoned"))?;
        let vista = slot
            .as_mut()
            .ok_or_else(|| error!("This handle's Vista was already used", verb = verb))?;
        let Some(shell) = vista.source.clone_shell() else {
            f(vista)?;
            drop(slot);
            return Ok(Handle {
                base: Base::Vista(source),
                ..self.clone()
            });
        };
        let mut copy = Vista::new(vista.name(), shell);
        drop(slot);
        f(&mut copy)?;
        let extended = Handle {
            base: Base::Vista(Arc::new(Mutex::new(Some(copy)))),
            ..self.clone()
        };
        if let Some(latest) = &self.latest {
            // The recorded copy holds no `latest`, so the cell never owns itself.
            *latest
                .lock()
                .map_err(|_| error!("table handle mutex poisoned"))? = Some(Handle {
                latest: None,
                ..extended.clone()
            });
        }
        Ok(extended)
    }

    /// Take a Vista base out of the handle, ignoring the steps.
    pub(crate) fn take_base(&self) -> Result<Vista> {
        match &self.base {
            Base::Vista(cell) => cell
                .lock()
                .map_err(|_| error!("table handle mutex poisoned"))?
                .take()
                .ok_or_else(|| error!("this handle's Vista was already used")),
            Base::Name(name) => Err(error!(
                "This table handle has no Vista in hand",
                table = name
            )),
        }
    }

    fn resolve_through(&self, resolver: Option<&TargetResolver>, steps: usize) -> Result<Vista> {
        let mut vista = self.resolve_base(resolver.or(self.resolver.as_ref()))?;
        // The smallest `limit(n)` since the last `ref`: the rows a `ref`
        // step follows.
        let mut limit: Option<usize> = None;
        for step in &self.steps[..steps] {
            vista = apply(vista, step, limit)
                .with_context(|| error!("Step can't be applied", step = step))?;
            limit = match step {
                Step::Limit(n) => Some(limit.map_or(*n, |l| l.min(*n))),
                Step::Ref(_) => None,
                _ => limit,
            };
        }
        Ok(vista)
    }

    fn resolve_base(&self, resolver: Option<&TargetResolver>) -> Result<Vista> {
        match &self.base {
            Base::Name(name) => {
                let resolver = resolver.ok_or_else(|| {
                    error!(
                        "Table can't be resolved here: no tables are available",
                        table = name
                    )
                })?;
                resolver(name)
            }
            Base::Vista(cell) => {
                let mut slot = cell
                    .lock()
                    .map_err(|_| error!("table handle mutex poisoned"))?;
                let copy = slot
                    .as_ref()
                    .and_then(|v| v.source.clone_shell().map(|s| Vista::new(v.name(), s)));
                copy.or_else(|| slot.take()).ok_or_else(|| {
                    error!(
                        "this handle's backend can't be copied; build it again from `table(...)`"
                    )
                })
            }
        }
    }
}

/// Apply one step; `limit` caps the rows a `ref` step follows.
fn apply(mut vista: Vista, step: &Step, limit: Option<usize>) -> Result<Vista> {
    match step {
        Step::Where { col, op, value } => vista.add_condition(col.clone(), *op, value.clone())?,
        Step::Sort { col, dir } => vista.add_order(col, *dir)?,
        Step::Search(text) => vista.add_search(text.clone())?,
        Step::Limit(n) => vista.set_page_size(*n)?,
        Step::Ref(rel) => return traverse(&vista, rel, limit),
    }
    Ok(vista)
}
