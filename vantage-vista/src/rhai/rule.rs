//! The rule handle: what a rule script's `table` is.
//!
//! A rule script runs on a table before any Vista exists, so its handle is
//! not a [`Handle`](super::Handle). It narrows a [`RuleTarget`] in place and
//! has only verbs that narrow: `where`, a backend's `with_condition`, the
//! `before_*` hooks and `remove_column`. No read, write, `table(name)`,
//! `ref`, `sort`, `limit` or `search` verb exists on it, so a script can't
//! reach another table or loosen anything.
//!
//! Every verb is recorded as a [`RuleVerb`], so a caller can explain what a
//! rule did to the table.

use std::any::Any;
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_core::{Context, Result, error};
use vantage_rhai::rhai::{AST, Dynamic, Engine, EvalAltResult, FnPtr};
use vantage_rhai::{Env, Host};

use super::convert::{cbor_to_dynamic, dynamic_to_cbor};
use super::eval::compile;
use super::read::RhaiResult;
use crate::FilterOp;

/// A verb a rule script called, as the access explanation records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleVerb {
    Where { column: String, described: String },
    Condition { described: String },
    Hook { on: HookOn },
    RemoveColumn { column: String },
}

/// The write a rule hook runs before.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookOn {
    BeforeInsert,
    BeforeUpdate,
    BeforeSave,
    BeforeDelete,
}

/// What a rule hook receives.
///
/// A closure registered with `before_delete(|id| …)` gets the id from both
/// [`Id`](Self::Id) and [`Delete`](Self::Delete); one written as
/// `before_delete(|id, row| …)` also gets the row from `Delete`, and unit
/// from `Id`.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum RuleHookInput {
    /// The record about to be written. On an update, a rule target hands
    /// over the full row: the stored row with the written fields on top.
    Record(CborValue),
    /// The id of the row about to be deleted.
    Id(CborValue),
    /// The id of the row about to be deleted, and the row as stored.
    Delete { id: CborValue, row: CborValue },
}

/// A rule hook: `Err(message)` refuses the write with the script's message.
pub type RuleHookFn = Arc<dyn Fn(RuleHookInput) -> std::result::Result<(), String> + Send + Sync>;

/// The table a rule script narrows. Each method narrows it in place.
pub trait RuleTarget: Send {
    /// Narrow to rows where `column <op> value`; returns the condition as
    /// text for people.
    fn where_value(&mut self, column: &str, op: FilterOp, value: CborValue) -> Result<String>;
    /// Narrow by a backend's own condition, built by its `with_condition`;
    /// `preview` is its rendered form. Returns the condition as text for people.
    fn native_condition(
        &mut self,
        condition: Box<dyn Any + Send>,
        preview: String,
    ) -> Result<String>;
    /// Run `run` before the writes `on` names.
    fn hook(&mut self, on: HookOn, run: RuleHookFn) -> Result<()>;
    /// Take `column` out of the table.
    fn remove_column(&mut self, column: &str) -> Result<()>;
}

/// The engine and script a hook closure is called on after the script ends.
struct RuleRuntime {
    engine: Arc<Engine>,
    ast: Arc<AST>,
}

/// The `table` of a rule script: a shared [`RuleTarget`] and the verbs called
/// on it. Clones share both.
#[derive(Clone)]
pub struct RuleHandle {
    target: Arc<Mutex<Box<dyn RuleTarget>>>,
    verbs: Arc<Mutex<Vec<RuleVerb>>>,
    runtime: Arc<Mutex<Option<Arc<RuleRuntime>>>>,
}

impl RuleHandle {
    pub fn new(target: Box<dyn RuleTarget>) -> Self {
        Self {
            target: Arc::new(Mutex::new(target)),
            verbs: Arc::new(Mutex::new(Vec::new())),
            runtime: Arc::new(Mutex::new(None)),
        }
    }

    /// The verbs called so far, in order.
    pub fn verbs(&self) -> Vec<RuleVerb> {
        self.verbs.lock().unwrap().clone()
    }

    /// Narrow by a backend's own condition; a backend's `with_condition`
    /// verb calls this.
    pub fn native_condition(&self, condition: Box<dyn Any + Send>, preview: String) -> Result<()> {
        let described = self.target().native_condition(condition, preview)?;
        self.record(RuleVerb::Condition { described });
        Ok(())
    }

    fn target(&self) -> std::sync::MutexGuard<'_, Box<dyn RuleTarget>> {
        self.target.lock().unwrap()
    }

    fn record(&self, verb: RuleVerb) {
        self.verbs.lock().unwrap().push(verb);
    }

    fn where_value(&self, column: &str, op: FilterOp, value: Dynamic) -> RhaiResult<RuleHandle> {
        let value = dynamic_to_cbor(value)?;
        let described = self
            .target()
            .where_value(column, op, value)
            .map_err(|e| e.to_string())?;
        self.record(RuleVerb::Where {
            column: column.to_string(),
            described,
        });
        Ok(self.clone())
    }

    fn hook(&self, on: HookOn, closure: FnPtr) -> RhaiResult<RuleHandle> {
        let runtime = self
            .runtime
            .lock()
            .unwrap()
            .clone()
            .ok_or("rule hooks need a script run by eval_rule_script")?;
        let takes_row = on == HookOn::BeforeDelete && arity(&closure, &runtime.ast) == Some(2);
        let run: RuleHookFn = Arc::new(move |input| {
            let (first, row) = match input {
                RuleHookInput::Record(value) | RuleHookInput::Id(value) => {
                    (cbor_to_dynamic(&value), Dynamic::UNIT)
                }
                RuleHookInput::Delete { id, row } => (cbor_to_dynamic(&id), cbor_to_dynamic(&row)),
            };
            let outcome = if takes_row {
                closure.call::<Dynamic>(&runtime.engine, &runtime.ast, (first, row))
            } else {
                closure.call::<Dynamic>(&runtime.engine, &runtime.ast, (first,))
            };
            outcome.map(|_| ()).map_err(|e| refusal(*e))
        });
        self.target().hook(on, run).map_err(|e| e.to_string())?;
        self.record(RuleVerb::Hook { on });
        Ok(self.clone())
    }

    fn remove_column(&self, column: &str) -> RhaiResult<RuleHandle> {
        self.target()
            .remove_column(column)
            .map_err(|e| e.to_string())?;
        self.record(RuleVerb::RemoveColumn {
            column: column.to_string(),
        });
        Ok(self.clone())
    }
}

/// How many arguments `closure` takes: its script function's parameters,
/// less the variables it captured (Rhai curries those in front). `None`
/// for a pointer to a function the script doesn't define.
fn arity(closure: &FnPtr, ast: &AST) -> Option<usize> {
    let function = ast.iter_functions().find(|f| f.name == closure.fn_name())?;
    function.params.len().checked_sub(closure.curry().len())
}

/// The message a hook closure refused with: what it threw, or, when it
/// failed some other way, the failure, so a broken rule refuses the write
/// rather than letting it through.
fn refusal(err: EvalAltResult) -> String {
    match err {
        EvalAltResult::ErrorRuntime(value, _) => value
            .clone()
            .into_string()
            .unwrap_or_else(|_| value.to_string()),
        EvalAltResult::ErrorInFunctionCall(_, _, inner, _) => refusal(*inner),
        other => other.to_string(),
    }
}

/// Register the rule handle (Rhai type name `Table`) and its verbs:
/// `where(column, value)`, `where(column, op, value)`, `before_insert(fn)`,
/// `before_update(fn)`, `before_save(fn)`, `before_delete(fn)` and
/// `remove_column(name)`. Each returns the handle, so calls chain.
pub fn register_rule_vocab(engine: &mut Engine) {
    engine.register_type_with_name::<RuleHandle>("Table");

    engine.register_fn(
        "where",
        |h: &mut RuleHandle, column: &str, value: Dynamic| {
            h.where_value(column, FilterOp::Eq, value)
        },
    );
    engine.register_fn(
        "where",
        |h: &mut RuleHandle, column: &str, op: &str, value: Dynamic| -> RhaiResult<RuleHandle> {
            let op = FilterOp::parse(op).ok_or_else(|| {
                format!(
                    "unknown operator \"{op}\" in where (expected eq/ne/gt/gte/lt/lte/in/not_in/like)"
                )
            })?;
            h.where_value(column, op, value)
        },
    );
    for (name, on) in [
        ("before_insert", HookOn::BeforeInsert),
        ("before_update", HookOn::BeforeUpdate),
        ("before_save", HookOn::BeforeSave),
        ("before_delete", HookOn::BeforeDelete),
    ] {
        engine.register_fn(name, move |h: &mut RuleHandle, closure: FnPtr| {
            h.hook(on, closure)
        });
    }
    engine.register_fn("remove_column", |h: &mut RuleHandle, column: &str| {
        h.remove_column(column)
    });
}

/// Run a rule script with `handle` as `table`, on a host that registered
/// [`register_rule_vocab`] (and a backend's rule conditions). `env` carries
/// the rest of its scope (`session`, `app`). Hook closures keep the host's
/// engine and the compiled script, and run on them when the write comes.
pub fn eval_rule_script(host: &Host, code: &str, env: Env, handle: &RuleHandle) -> Result<()> {
    const WHAT: &str = "rhai rule script";
    let script = compile(host, WHAT, code)?;
    let ast = script
        .ast()
        .ok_or_else(|| error!("rhai rule script compiled to no single AST", script = WHAT))?;
    *handle.runtime.lock().unwrap() = Some(Arc::new(RuleRuntime {
        engine: host.engine().clone(),
        ast: Arc::new(ast.clone()),
    }));
    script
        .run(&env.var("table", Dynamic::from(handle.clone())))
        .context(error!("rhai script failed", script = WHAT))
}
