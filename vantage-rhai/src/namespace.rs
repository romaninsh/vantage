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
