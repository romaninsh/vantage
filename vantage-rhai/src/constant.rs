//! Engine-wide constants: a name every script on an engine reads without it
//! being pushed into each evaluation's [`Env`](crate::Env).

use rhai::{Dynamic, Engine, Module};

/// Make `name` read as `value` in every script `engine` evaluates, including
/// inside script-defined functions.
///
/// It is a true constant. The value lives in a global module registered on
/// the engine, and rhai reads such variables in two places (rhai 1.25 and
/// 1.26):
/// - at compile time, the optimizer seeds its constants from the engine's
///   global modules (`Engine::optimize_top_level` in rhai's
///   `src/optimizer.rs`) and inlines the value;
/// - at run time, with optimization off, variable lookup falls back to them
///   when a name is in no scope (`Engine::search_scope_only` in
///   `src/eval/expr.rs`).
///
/// The doc of `Engine::register_global_module` still says such variables
/// are ignored; that line is stale. The tests below pin the behaviour, so a
/// rhai upgrade that changes it fails here first.
///
/// Because the value is inlined when the script compiles, a `let` of the
/// same name in the script shadows it, but an [`Env`](crate::Env) variable
/// does not. Pass a per-evaluation value through `Env` under another name.
/// A [`Host`](crate::Host)'s resolver yields for names it doesn't know, so
/// the two compose.
pub fn register_constant(engine: &mut Engine, name: &str, value: impl Into<Dynamic>) {
    let mut module = Module::new();
    module.set_var(name, value.into());
    engine.register_global_module(module.into());
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{Block, Env, Host, Limits, Lookup, Resolver};

    fn host() -> Host {
        Host::builder(Limits::Ui)
            .vocab_fn(|e| register_constant(e, "answer", 42_i64))
            .build()
    }

    fn eval(host: &Host, code: &str, env: &Env) -> i64 {
        host.compile(&Block::from(code))
            .and_then(|script| script.eval(env))
            .unwrap()
            .as_int()
            .unwrap()
    }

    #[test]
    fn a_constant_reads_without_an_env() {
        assert_eq!(eval(&host(), "answer + 1", &Env::new()), 43);
    }

    #[test]
    fn a_constant_reads_inside_a_script_function() {
        let code = "fn twice() { answer * 2 } twice()";
        assert_eq!(eval(&host(), code, &Env::new()), 84);
    }

    #[test]
    fn a_let_shadows_the_constant_but_an_env_variable_does_not() {
        assert_eq!(eval(&host(), "let answer = 1; answer", &Env::new()), 1);
        let env = Env::new().var("answer", 7_i64);
        assert_eq!(eval(&host(), "answer", &env), 42);
    }

    #[test]
    fn a_constant_composes_with_a_resolver() {
        struct Ns;
        impl Resolver for Ns {
            fn resolve(&self, path: &str) -> Lookup {
                match path {
                    "n" => Lookup::Leaf(Dynamic::from(2_i64)),
                    _ => Lookup::Unknown,
                }
            }
        }
        let env = Env::new().resolver(Arc::new(Ns));
        assert_eq!(eval(&host(), "answer + n", &env), 44);
    }
}
