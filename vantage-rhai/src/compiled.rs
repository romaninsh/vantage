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
use crate::host::Host;
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
    Hole(Arc<AST>),
}

fn expr_ast(host: &Host, src: &str, cached: bool) -> Result<Arc<AST>> {
    let compile = |e: &Engine| e.compile_expression(src);
    if cached {
        host.ast(&format!("e:{src}"), src, compile)
    } else {
        host.ast_uncached(src, compile)
    }
}

fn script_ast(host: &Host, src: &str, cached: bool) -> Result<Arc<AST>> {
    let compile = |e: &Engine| e.compile(src);
    if cached {
        host.ast(&format!("s:{src}"), src, compile)
    } else {
        host.ast_uncached(src, compile)
    }
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
        let mut parts = Vec::new();
        for part in template::split(Template::src(self))? {
            parts.push(match part {
                Part::Lit(s) => TPart::Lit(s),
                Part::Hole(h) => TPart::Hole(expr_ast(host, h.trim(), cached)?),
            });
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
        env: &Env,
        resolver: Option<Arc<dyn Resolver>>,
    ) -> Result<Dynamic> {
        let mut scope = env.scope(resolver);
        self.engine
            .eval_ast_with_scope::<Dynamic>(&mut scope, ast)
            .map_err(|e| RhaiError::from_eval(&self.src, e))
    }

    fn eval_with(&self, env: &Env, resolver: Option<Arc<dyn Resolver>>) -> Result<Dynamic> {
        match &self.pieces {
            Pieces::One(ast) => self.eval_ast(ast, env, resolver),
            Pieces::Parts(parts) => {
                if let [TPart::Hole(ast)] = parts.as_slice() {
                    return self.eval_ast(ast, env, resolver);
                }
                let mut out = String::new();
                for part in parts {
                    match part {
                        TPart::Lit(s) => out.push_str(s),
                        TPart::Hole(ast) => {
                            out.push_str(&render(&self.eval_ast(ast, env, resolver.clone())?))
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
    pub fn run(&self, env: &Env) -> Result<()> {
        self.eval_dynamic(env).map(|_| ())
    }

    /// The script's final expression value, unit when there is none.
    pub fn eval(&self, env: &Env) -> Result<Dynamic> {
        self.eval_dynamic(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use crate::resolver::tests::MapResolver;
    use std::collections::HashMap;

    fn host() -> Host {
        Host::builder(Limits::Ui).build()
    }

    fn resolver() -> Arc<dyn Resolver> {
        let mut m = HashMap::new();
        m.insert("app.currency".to_string(), Dynamic::from("USD"));
        m.insert("app.show".to_string(), Dynamic::from(true));
        m.insert("args.id".to_string(), Dynamic::from(42_i64));
        Arc::new(MapResolver(m))
    }

    #[test]
    fn expr_evaluates_and_strips_one_wrapper() {
        let h = host();
        let env = Env::new().var("x", 2_i64);
        let c = h.compile(&Expr::from("${ x * 21 }")).unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 42);
        assert!(
            h.compile(&Expr::from("${x}${x}")).is_err(),
            "two holes are not a wrapper"
        );
    }

    #[test]
    fn eval_bool_rejects_non_bool() {
        let h = host();
        let env = Env::new();
        let c = h.compile(&Expr::from("1 + 1")).unwrap();
        assert!(matches!(
            c.eval_bool(&env),
            Err(RhaiError::WrongType {
                expected: "bool",
                ..
            })
        ));
        let c = h.compile(&Expr::from("1 < 2")).unwrap();
        assert!(c.eval_bool(&env).unwrap());
    }

    #[test]
    fn expr_rejects_statements() {
        let h = host();
        assert!(matches!(
            h.compile(&Expr::from("let a = 1; a")),
            Err(RhaiError::Syntax(_))
        ));
    }

    #[test]
    fn template_literal_single_and_mixed() {
        let h = host();
        let env = Env::new().var("n", 7_i64).var("u", ());
        let lit = h.compile(&Template::from("plain")).unwrap();
        assert!(lit.is_literal());
        assert_eq!(lit.eval(&env).unwrap().to_string(), "plain");

        let single = h.compile(&Template::from("${ n }")).unwrap();
        assert!(!single.is_literal());
        assert_eq!(single.eval(&env).unwrap().as_int().unwrap(), 7);

        let mixed = h.compile(&Template::from("n=${n} u=[${u}]")).unwrap();
        assert_eq!(mixed.eval(&env).unwrap().to_string(), "n=7 u=[]");
    }

    #[test]
    fn block_runs_and_exposes_final_value() {
        let h = host();
        let env = Env::new();
        let c = h.compile(&Block::from("let a = 40; a + 2")).unwrap();
        c.run(&env).unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 42);
        let c = h.compile(&Block::from("let a = 1;")).unwrap();
        assert!(c.eval(&env).unwrap().is_unit());
    }

    #[test]
    fn discovery_records_leaf_paths_and_freezes() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let c = h.compile(&Expr::from("app.currency")).unwrap();
        assert!(!c.is_discovered());
        assert!(c.read_set().is_empty());
        let c = c.discover(&env).unwrap();
        assert!(c.is_discovered());
        assert_eq!(
            c.read_set().iter().collect::<Vec<_>>(),
            vec!["app.currency"]
        );
        assert_eq!(c.eval(&env).unwrap().to_string(), "USD");
    }

    #[test]
    fn template_discovery_unions_holes() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let c = h
            .compile(&Template::from("Order ${args.id} in ${app.currency}"))
            .unwrap()
            .discover(&env)
            .unwrap();
        assert_eq!(c.read_set().len(), 2);
        assert_eq!(c.eval(&env).unwrap().to_string(), "Order 42 in USD");
    }

    #[test]
    fn unknown_name_fails_at_discover_not_compile() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let c = h.compile(&Expr::from("app.currencyy")).unwrap();
        match c.discover(&env) {
            Err(RhaiError::UnknownName { path, .. }) => assert_eq!(path, "app.currencyy"),
            other => panic!("{:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn pushed_vars_shadow_resolver_in_env() {
        let h = host();
        let env = Env::new().resolver(resolver()).var("args", 1_i64);
        let c = h
            .compile(&Expr::from("args"))
            .unwrap()
            .discover(&env)
            .unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 1);
        assert!(c.read_set().is_empty());
    }

    #[test]
    fn discover_value_returns_the_first_result() {
        let h = host();
        let env = Env::new().resolver(resolver());
        let (c, v) = h
            .compile(&Block::from("let x = args.id; x * 2"))
            .unwrap()
            .discover_value(&env)
            .unwrap();
        assert_eq!(v.as_int().unwrap(), 84);
        assert_eq!(c.read_set().iter().collect::<Vec<_>>(), vec!["args.id"]);
    }

    #[test]
    fn limit_error_surfaces_as_limit_exceeded() {
        let h = host();
        let c = h.compile(&Block::from("while true {}")).unwrap();
        assert!(matches!(
            c.run(&Env::new()),
            Err(RhaiError::LimitExceeded {
                limit: "operations",
                ..
            })
        ));
    }

    #[test]
    fn host_is_shared_across_threads() {
        let h = Arc::new(host());
        let c = Arc::new(h.compile(&Expr::from("x * 2")).unwrap());
        let handles: Vec<_> = (1..=4_i64)
            .map(|i| {
                let c = c.clone();
                std::thread::spawn(move || {
                    c.eval(&Env::new().var("x", i)).unwrap().as_int().unwrap()
                })
            })
            .collect();
        let got: Vec<i64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(got, vec![2, 4, 6, 8]);
    }
}
