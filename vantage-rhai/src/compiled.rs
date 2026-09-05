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
// A script's final expression is its value, so the typed accessors apply.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::Limits;
    use crate::resolver::Lookup;
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
    fn template_hole_error_reports_template_coordinates() {
        let h = host();
        // The fault is on line 2, well to the right — not line 1 column 1.
        let t = h
            .compile(&Template::from(
                "short\nthis is a longer line ${ nosuchfn(1) }",
            ))
            .unwrap();
        let err = t.eval(&Env::new()).unwrap_err();
        let RhaiError::Runtime(l) = err else {
            panic!("expected Runtime, got {err:?}")
        };
        assert_eq!(l.line, 2, "line must index the template, not the hole");
        assert_eq!(l.column, 26, "column must offset by where the hole starts");
        let shown = l.to_string();
        assert!(shown.contains("this is a longer line"), "{shown}");
        // The position is printed once, by Located — not also by rhai.
        assert_eq!(shown.matches("line 2").count(), 1, "{shown}");
    }

    #[test]
    fn template_syntax_error_reports_template_coordinates() {
        // Compile-time counterpart: a malformed hole must point into the
        // template and carry the template as its source, so a validator can
        // name the YAML key.
        let h = host();
        let err = h
            .compile(&Template::from(
                "line one
value is ${ 1 + } here",
            ))
            .unwrap_err();
        let RhaiError::Syntax(l) = &err else {
            panic!("expected Syntax, got {err:?}")
        };
        assert_eq!(l.line, 2, "line must index the template");
        assert!(
            l.column >= 13,
            "column must sit inside the hole, got {}",
            l.column
        );
        assert!(err.src().contains("line one"), "src must be the template");
    }

    #[test]
    fn unknown_name_inside_a_script_fn_is_still_unknown_name() {
        // rhai wraps a failure inside `fn` in ErrorInFunctionCall; the wrapper
        // must not hide the real cause from classification.
        let h = host();
        let env = Env::new().resolver(resolver());
        let c = h.compile(&Block::from("fn f() { app.nope } f()")).unwrap();
        match c.eval(&env) {
            Err(RhaiError::UnknownName { path, .. }) => assert_eq!(path, "app"),
            other => panic!("expected UnknownName, got {:?}", other.map(|_| ())),
        }
    }

    #[test]
    fn resolver_is_consulted_once_per_name() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counting(AtomicUsize);
        impl Resolver for Counting {
            fn resolve(&self, path: &str) -> Lookup {
                self.0.fetch_add(1, Ordering::SeqCst);
                match path {
                    "app" => Lookup::Namespace,
                    "app.n" => Lookup::Leaf(Dynamic::from(1_i64)),
                    _ => Lookup::Unknown,
                }
            }
        }

        let counter = Arc::new(Counting(AtomicUsize::new(0)));
        let env = Env::new().resolver(counter.clone() as Arc<dyn Resolver>);
        let c = host().compile(&Expr::from("app.n")).unwrap();
        assert_eq!(c.eval(&env).unwrap().as_int().unwrap(), 1);
        // `app` then `app.n`. Resolving twice would make this 3.
        assert_eq!(counter.0.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn multi_hole_template_shares_one_scope() {
        // Each hole sees the same pushed vars, and evaluation order is stable.
        let h = host();
        let env = Env::new().var("a", 1_i64).var("b", 2_i64);
        let t = h.compile(&Template::from("${a}-${b}-${a + b}")).unwrap();
        assert_eq!(t.eval(&env).unwrap().to_string(), "1-2-3");
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
