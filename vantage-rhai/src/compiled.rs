//! Compiled slots and the per-evaluation environment.
//!
//! Trust model: "eval" here is the embedded Rhai engine running scripts an
//! author wrote in this project's own inventory YAML — the same trust level as
//! the YAML itself, never third-party input. Rhai has no filesystem, network or
//! process access of its own; a host only sees what its [`Vocab`](crate::Vocab)
//! registers. Every engine additionally carries a [`Limits`](crate::Limits)
//! profile, so a mistyped loop bounds out instead of hanging the caller.

use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::sync::Arc;

use rhai::{AST, Dynamic, Engine};

use crate::error::{Result, RhaiError};
use crate::host::{Host, Mode};
use crate::resolver::{ENV_KEY, EnvHandle, RecordingResolver, Resolver};
use crate::slot::{Block, Expr, Template};
use crate::template::{self, Part};

/// Per-evaluation inputs: pushed variables plus an optional lazy resolver.
#[derive(Default, Clone)]
pub struct Env {
    vars: Vec<(String, Dynamic)>,
    resolver: Option<Arc<dyn Resolver>>,
}

impl Env {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn var(mut self, name: impl Into<String>, value: impl Into<Dynamic>) -> Self {
        self.vars.push((name.into(), value.into()));
        self
    }

    pub fn resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }

    fn scope(&self, resolver: Option<Arc<dyn Resolver>>) -> rhai::Scope<'static> {
        let mut scope = rhai::Scope::new();
        for (name, value) in &self.vars {
            scope.push_dynamic(name.clone(), value.clone());
        }
        if let Some(r) = resolver {
            scope.push_constant(ENV_KEY, EnvHandle(r));
        }
        scope
    }
}

mod sealed {
    pub trait Sealed {}
}

/// What a slot kind knows: how to compile itself into pieces.
pub trait Slot: sealed::Sealed + Sized {
    #[doc(hidden)]
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces>;
    #[doc(hidden)]
    fn src(&self) -> &str;
}

#[doc(hidden)]
pub enum Pieces {
    One(Arc<AST>),
    Parts(Vec<TPart>),
}

#[doc(hidden)]
pub enum TPart {
    Lit(String),
    Hole {
        ast: Arc<AST>,
        /// 1-based (line, column) where this hole's expression starts in the
        /// template, so an error inside it reports template coordinates.
        at: (usize, usize),
    },
}

fn compile_ast(host: &Host, mode: Mode, src: &str, cached: bool) -> Result<Arc<AST>> {
    if cached {
        host.ast(mode, src)
    } else {
        host.ast_uncached(mode, src)
    }
}

fn expr_ast(host: &Host, src: &str, cached: bool) -> Result<Arc<AST>> {
    compile_ast(host, Mode::Expression, src, cached)
}

fn script_ast(host: &Host, src: &str, cached: bool) -> Result<Arc<AST>> {
    compile_ast(host, Mode::Script, src, cached)
}

impl sealed::Sealed for Expr {}
impl Slot for Expr {
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces> {
        let inner = template::strip_single_wrapper(Expr::src(self));
        Ok(Pieces::One(expr_ast(host, inner, cached)?))
    }
    fn src(&self) -> &str {
        Expr::src(self)
    }
}

impl sealed::Sealed for Block {}
impl Slot for Block {
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces> {
        Ok(Pieces::One(script_ast(host, Block::src(self), cached)?))
    }
    fn src(&self) -> &str {
        Block::src(self)
    }
}

impl sealed::Sealed for Template {}
impl Slot for Template {
    fn pieces(&self, host: &Host, cached: bool) -> Result<Pieces> {
        let src = Template::src(self);
        let mut parts = Vec::new();
        // Byte cursor into `src`, so each hole knows its own line/column.
        let mut at = 0usize;
        for part in template::split(src)? {
            match part {
                Part::Lit(s) => {
                    at += s.len();
                    parts.push(TPart::Lit(s));
                }
                Part::Hole(h) => {
                    // `at` sits on the `$`; the expression begins after `${`
                    // plus whatever `trim` drops from the front.
                    let lead = h.len() - h.trim_start().len();
                    let expr_at = line_col(src, at + 2 + lead);
                    parts.push(TPart::Hole {
                        // A hole compiles alone, so a syntax error in it points
                        // into the hole; rebase it onto the template.
                        ast: expr_ast(host, h.trim(), cached)
                            .map_err(|e| e.rebase(src, expr_at))?,
                        at: expr_at,
                    });
                    at += 2 + h.len() + 1;
                }
            }
        }
        // A lone hole keeps its value (a map, a number) instead of rendering
        // to text. Whitespace around it must not change that: a YAML block
        // scalar leaves a newline after `${ … }`.
        let holes = parts
            .iter()
            .filter(|p| matches!(p, TPart::Hole { .. }))
            .count();
        let only_blank_text = parts
            .iter()
            .all(|p| !matches!(p, TPart::Lit(s) if !s.trim().is_empty()));
        if holes == 1 && only_blank_text {
            parts.retain(|p| matches!(p, TPart::Hole { .. }));
        }
        Ok(Pieces::Parts(parts))
    }
    fn src(&self) -> &str {
        Template::src(self)
    }
}

/// A compiled slot bound to the host's engine. Evaluate many times.
pub struct Compiled<S: Slot> {
    engine: Arc<Engine>,
    src: String,
    pieces: Pieces,
    read_set: BTreeSet<String>,
    discovered: bool,
    _kind: PhantomData<S>,
}

impl Host {
    pub fn compile<S: Slot>(&self, slot: &S) -> Result<Compiled<S>> {
        self.compile_inner(slot, true)
    }

    /// Bypass the AST cache (one-off scripts).
    pub fn compile_uncached<S: Slot>(&self, slot: &S) -> Result<Compiled<S>> {
        self.compile_inner(slot, false)
    }

    fn compile_inner<S: Slot>(&self, slot: &S, cached: bool) -> Result<Compiled<S>> {
        Ok(Compiled {
            engine: self.engine().clone(),
            src: slot.src().to_string(),
            pieces: slot.pieces(self, cached)?,
            read_set: BTreeSet::new(),
            discovered: false,
            _kind: PhantomData,
        })
    }
}

/// 1-based (line, column) of a byte offset, counting columns in chars.
fn line_col(src: &str, at: usize) -> (usize, usize) {
    let line = src[..at].matches('\n').count() + 1;
    let start = src[..at].rfind('\n').map(|p| p + 1).unwrap_or(0);
    (line, src[start..at].chars().count() + 1)
}

impl<S: Slot> std::fmt::Debug for Compiled<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compiled")
            .field("kind", &std::any::type_name::<S>())
            .field("src", &self.src)
            .field("discovered", &self.discovered)
            .field("read_set", &self.read_set)
            .finish()
    }
}

fn render(value: &Dynamic) -> String {
    if value.is_unit() {
        String::new()
    } else {
        value.to_string()
    }
}

impl<S: Slot> Compiled<S> {
    pub fn src(&self) -> &str {
        &self.src
    }

    /// The engine this slot was compiled on. For callers that hand a value
    /// back to rhai themselves — a closure's `FnPtr::call` needs the engine
    /// and the AST that defined it.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The one compiled AST behind an `Expr` or `Block` (or a template that is
    /// a single hole). `None` for a template with text around its holes, which
    /// has one AST per hole and no single script to hand out.
    pub fn ast(&self) -> Option<&AST> {
        match &self.pieces {
            Pieces::One(ast) => Some(ast),
            Pieces::Parts(parts) => match parts.as_slice() {
                [TPart::Hole { ast, .. }] => Some(ast),
                _ => None,
            },
        }
    }

    /// Dotted paths read through the resolver during discovery. Empty until
    /// `discover` has run.
    pub fn read_set(&self) -> &BTreeSet<String> {
        &self.read_set
    }

    pub fn is_discovered(&self) -> bool {
        self.discovered
    }

    /// Evaluate once with reads recorded; freeze the read-set. An unknown name
    /// fails here, before anything renders.
    pub fn discover(self, env: &Env) -> Result<Self> {
        self.discover_value(env).map(|(c, _)| c)
    }

    /// Like `discover`, also returning that first evaluation's value.
    pub fn discover_value(mut self, env: &Env) -> Result<(Self, Dynamic)> {
        let recorder = env
            .resolver
            .clone()
            .map(|r| Arc::new(RecordingResolver::new(r)));
        let as_resolver: Option<Arc<dyn Resolver>> =
            recorder.clone().map(|r| r as Arc<dyn Resolver>);
        let value = self.eval_with(env, as_resolver)?;
        self.read_set = recorder.map(|r| r.take_reads()).unwrap_or_default();
        self.discovered = true;
        Ok((self, value))
    }

    fn eval_ast(
        &self,
        ast: &AST,
        scope: &mut rhai::Scope<'static>,
        remap: Option<(usize, usize)>,
    ) -> Result<Dynamic> {
        self.engine
            .eval_ast_with_scope::<Dynamic>(scope, ast)
            .map_err(|e| RhaiError::from_eval_at(&self.src, e, remap))
    }

    fn eval_with(&self, env: &Env, resolver: Option<Arc<dyn Resolver>>) -> Result<Dynamic> {
        // One scope per evaluation, not one per template hole. Holes compile
        // through `compile_expression`, so none of them can leave a binding
        // behind for the next.
        let mut scope = env.scope(resolver);
        match &self.pieces {
            Pieces::One(ast) => self.eval_ast(ast, &mut scope, None),
            Pieces::Parts(parts) => {
                if let [TPart::Hole { ast, at }] = parts.as_slice() {
                    return self.eval_ast(ast, &mut scope, Some(*at));
                }
                let mut out = String::new();
                for part in parts {
                    match part {
                        TPart::Lit(s) => out.push_str(s),
                        TPart::Hole { ast, at } => {
                            let v = self.eval_ast(ast, &mut scope, Some(*at))?;
                            out.push_str(&render(&v));
                        }
                    }
                }
                Ok(Dynamic::from(out))
            }
        }
    }

    fn eval_dynamic(&self, env: &Env) -> Result<Dynamic> {
        self.eval_with(env, env.resolver.clone())
    }
}

macro_rules! value_methods {
    ($kind:ty) => {
        impl Compiled<$kind> {
            pub fn eval(&self, env: &Env) -> Result<Dynamic> {
                self.eval_dynamic(env)
            }

            pub fn eval_bool(&self, env: &Env) -> Result<bool> {
                let v = self.eval_dynamic(env)?;
                v.as_bool()
                    .map_err(|_| RhaiError::wrong_type(&self.src, "bool", &v))
            }

            pub fn eval_as<T: Clone + Send + Sync + 'static>(&self, env: &Env) -> Result<T> {
                let v = self.eval_dynamic(env)?;
                v.clone()
                    .try_cast::<T>()
                    .ok_or_else(|| RhaiError::wrong_type(&self.src, std::any::type_name::<T>(), &v))
            }
        }
    };
}

value_methods!(Expr);
value_methods!(Template);
value_methods!(Block);

impl Compiled<Template> {
    /// No holes: a static value.
    pub fn is_literal(&self) -> bool {
        match &self.pieces {
            Pieces::Parts(parts) => parts.iter().all(|p| matches!(p, TPart::Lit(_))),
            Pieces::One(_) => false,
        }
    }
}

impl Compiled<Block> {
    /// Run for effect; the final value (if any) is discarded. `eval` returns
    /// it instead, unit when the script ends on a statement.
    pub fn run(&self, env: &Env) -> Result<()> {
        self.eval_dynamic(env).map(|_| ())
    }
}
