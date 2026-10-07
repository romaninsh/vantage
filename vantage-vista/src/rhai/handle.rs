//! The immutable table handle scripts narrow: a base plus a list of steps.
//!
//! Every narrowing verb returns a new [`Handle`] that shares its prefix with
//! the one it came from, so a handle stored in a variable never changes. A
//! [`Vista`] is only built when a terminal asks for one, through
//! [`Handle::resolve`].
//!
//! The exception is `self` in a `modify:` or augment script ([`Handle::over`]):
//! every verb called on it, or on a handle derived from it, builds on what the
//! earlier statements produced, so `self.a(); self.where(..);` applies both.

use std::sync::{Arc, Mutex, PoisonError};

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
    /// handle any verb produced from `self`. The next verb builds on it, and a
    /// script that ends on a statement (`self.with_condition(..);`) yields it.
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

    /// The latest handle a verb derived from this one's `self`, if any.
    pub(crate) fn latest_extended(&self) -> Option<Handle> {
        self.latest
            .as_ref()?
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The handle the next verb builds on: the latest one derived from `self`,
    /// else this one.
    fn current(&self) -> Handle {
        self.latest_extended().unwrap_or_else(|| self.clone())
    }

    /// Record `handle` as the latest derived from `self` (when this handle has
    /// a `self`) and return it sharing that record.
    fn record(&self, handle: Handle) -> Handle {
        if let Some(cell) = &self.latest {
            // The recorded copy holds no `latest`, so the cell never owns itself.
            *cell.lock().unwrap_or_else(PoisonError::into_inner) = Some(Handle {
                latest: None,
                ..handle.clone()
            });
        }
        Handle {
            latest: self.latest.clone(),
            ..handle
        }
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

    /// A new handle with `step` appended; `self` is left as it was. On a
    /// handle derived from a script's `self`, the step goes after the latest
    /// handle derived from it.
    pub(crate) fn push(&self, step: Step) -> Handle {
        let from = self.current();
        let mut steps = Vec::with_capacity(from.steps.len() + 1);
        steps.extend(from.steps.iter().cloned());
        steps.push(step);
        self.record(Handle {
            steps: Arc::new(steps),
            ..from
        })
    }

    /// Build the Vista: resolver(name) or a clone_shell of the base, then apply
    /// steps. Without `resolver`, the one `table(name)` was given is used.
    pub fn resolve(&self, resolver: Option<&TargetResolver>) -> Result<Vista> {
        self.resolve_through(resolver, self.steps.len())
    }

    /// The Vista writes go to: the whole narrowed handle — `where`, `search`
    /// and `ref` all define the set, so a write only touches rows in it.
    /// `sort` doesn't change membership and is skipped. `limit` defines rows
    /// by position, not by condition, so a handle with one can't be written
    /// through.
    pub(crate) fn write_target(&self, resolver: Option<&TargetResolver>) -> Result<Vista> {
        if self.steps.iter().any(|s| matches!(s, Step::Limit(_))) {
            return Err(
                error!("writes need a set defined by conditions; drop limit(n)")
                    .mark_incorrect_usage(),
            );
        }
        let mut vista = self.resolve_base(resolver.or(self.resolver.as_ref()))?;
        for step in self.steps.iter() {
            if matches!(step, Step::Sort { .. }) {
                continue;
            }
            vista = apply(vista, step, None)
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

    /// A handle over a changed copy of the Vista base, keeping the steps;
    /// `self` is left as it was. On a handle derived from a script's `self`,
    /// the change goes onto the latest handle derived from it. Backend
    /// extension verbs (`verb`) use this: they need a Vista in hand, so a
    /// handle built from `table(name)`, or one past a `ref`, is an error naming
    /// the verb. A backend that can't be copied is changed in place.
    pub fn with_base_vista(
        &self,
        verb: &str,
        f: impl FnOnce(&mut Vista) -> Result<()>,
    ) -> Result<Handle> {
        let from = self.current();
        let Base::Vista(source) = &from.base else {
            return Err(error!(
                "Extension verb only works on `self`, not on a handle from `table(...)`",
                verb = verb
            ));
        };
        if from.steps.iter().any(|s| matches!(s, Step::Ref(_))) {
            return Err(error!(
                "Extension verb can't follow `ref(...)`",
                verb = verb
            ));
        }
        let mut slot = source
            .lock()
            .map_err(|_| error!("Table handle mutex poisoned", verb = verb))?;
        let vista = slot
            .as_mut()
            .ok_or_else(|| error!("This handle's Vista was already used", verb = verb))?;
        let base = match vista.source.clone_shell() {
            Some(shell) => {
                let mut copy = Vista::new(vista.name(), shell);
                copy.narrowed = vista.narrowed;
                drop(slot);
                f(&mut copy)?;
                Base::Vista(Arc::new(Mutex::new(Some(copy))))
            }
            None => {
                f(vista)?;
                drop(slot);
                from.base.clone()
            }
        };
        Ok(self.record(Handle { base, ..from }))
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
                let copy = slot.as_ref().and_then(|v| {
                    v.source.clone_shell().map(|s| {
                        let mut copy = Vista::new(v.name(), s);
                        copy.narrowed = v.narrowed;
                        copy
                    })
                });
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
