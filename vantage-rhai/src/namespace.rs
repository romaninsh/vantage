//! A backend's free functions and constants under one rhai namespace, so a
//! script spells them `surreal::ident("name")` or `sql::count(..)` and they
//! can't shadow the data vocabulary's verbs (`table`, `where`, `first`, ..).
//!
//! The namespace is a rhai static module (`Engine::register_static_module`).
//! Its functions are registered `FnNamespace::Internal`, so they answer only
//! a qualified call: `surreal::ident(..)`, never `ident(..)` or a method call.
//! Methods and operators on a backend's own types don't belong here; they
//! dispatch on their receiver type and stay on the engine.

use rhai::{Engine, FuncRegistration, Module, RhaiNativeFunc, Variant};

/// A static module under construction. Add functions with [`func`] and
/// constants with [`var`], then [`register`] it on an engine.
///
/// [`func`]: Namespace::func
/// [`var`]: Namespace::var
/// [`register`]: Namespace::register
pub struct Namespace {
    name: String,
    module: Module,
}

impl Namespace {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module: Module::new(),
        }
    }

    /// Add `func` as `<namespace>::<name>`. Registering the same name again
    /// with other parameter types adds an overload, as `Engine::register_fn`
    /// does.
    pub fn func<A, const N: usize, const X: bool, R, const F: bool>(
        &mut self,
        name: &str,
        func: impl RhaiNativeFunc<A, N, X, R, F> + Send + Sync + 'static,
    ) -> &mut Self
    where
        A: 'static,
        R: Variant + Clone,
    {
        FuncRegistration::new(name).set_into_module(&mut self.module, func);
        self
    }

    /// Add a constant read as `<namespace>::<name>`, in any script on the
    /// engine and inside script-defined functions.
    pub fn var(&mut self, name: &str, value: impl Variant + Clone) -> &mut Self {
        self.module.set_var(name, value);
        self
    }

    /// Register the namespace on `engine`. A namespace registered earlier
    /// under the same name is replaced.
    pub fn register(self, engine: &mut Engine) {
        engine.register_static_module(self.name, self.module.into());
    }
}

#[cfg(test)]
mod tests {
    use rhai::Dynamic;

    use super::*;
    use crate::{Block, Env, Host, Limits};

    fn host() -> Host {
        Host::builder(Limits::Ui)
            .vocab_fn(|engine| {
                engine.register_fn("first", |n: i64| n + 1);
                let mut ns = Namespace::new("demo");
                ns.func("first", |n: i64| n * 10)
                    .func("first", |a: i64, b: i64| a * b)
                    .func("half", |n: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
                        if n % 2 == 0 {
                            Ok(n / 2)
                        } else {
                            Err("odd".into())
                        }
                    })
                    .var("answer", 42_i64);
                ns.register(engine);
            })
            .build()
    }

    fn eval(code: &str) -> Result<Dynamic, String> {
        host()
            .compile(&Block::from(code))
            .and_then(|script| script.eval(&Env::new()))
            .map_err(|e| e.to_string())
    }

    fn int(code: &str) -> i64 {
        eval(code).unwrap().as_int().unwrap()
    }

    #[test]
    fn a_namespaced_function_does_not_shadow_the_global_one() {
        assert_eq!(int("demo::first(2)"), 20);
        assert_eq!(int("first(2)"), 3);
        assert_eq!(int("demo::first(2, 3)"), 6);
    }

    #[test]
    fn a_namespaced_function_is_not_a_global_or_a_method() {
        let err = eval("half(4)").unwrap_err();
        assert!(err.contains("half"), "{err}");
        let err = eval("4.half()").unwrap_err();
        assert!(err.contains("half"), "{err}");
    }

    #[test]
    fn a_fallible_function_throws() {
        assert_eq!(int("demo::half(4)"), 2);
        assert!(eval("demo::half(3)").unwrap_err().contains("odd"));
    }

    #[test]
    fn registering_a_name_again_replaces_the_module() {
        let host = Host::builder(Limits::Ui)
            .vocab_fn(|engine| {
                let mut first = Namespace::new("demo");
                first.func("one", || 1_i64).func("only_first", || 0_i64);
                first.register(engine);
                let mut second = Namespace::new("demo");
                second.func("one", || 2_i64);
                second.register(engine);
            })
            .build();
        let eval = |code: &str| {
            host.compile(&Block::from(code))
                .and_then(|script| script.eval(&Env::new()))
                .map_err(|e| e.to_string())
        };
        assert_eq!(eval("demo::one()").unwrap().as_int().unwrap(), 2);
        let err = eval("demo::only_first()").unwrap_err();
        assert!(err.contains("only_first"), "{err}");
    }

    #[test]
    fn a_namespaced_constant_reads_anywhere() {
        assert_eq!(int("demo::answer + 1"), 43);
        assert_eq!(int("fn twice() { demo::answer * 2 } twice()"), 84);
        assert!(eval("answer").is_err());
    }
}
