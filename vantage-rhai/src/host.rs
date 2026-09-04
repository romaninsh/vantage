//! A configured engine plus a bounded AST cache. Built once per owner, shared.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rhai::{AST, Engine};

use crate::error::{Result, RhaiError};
use crate::limits::Limits;

/// A domain vocabulary: something that registers functions/types on an engine.
pub trait Vocab {
    fn register(&self, engine: &mut Engine);
}

/// Builds the engine eagerly: limits and the resolver hook go on first, then
/// each vocabulary registers in call order. `vocab_fn` therefore takes a plain
/// `FnOnce` with no `Send`/`'static` bounds, so a caller can move a resolver
/// or a limit into it.
pub struct HostBuilder {
    limits: Limits,
    engine: Engine,
}

impl HostBuilder {
    pub fn vocab(mut self, vocab: impl Vocab) -> Self {
        vocab.register(&mut self.engine);
        self
    }

    pub fn vocab_fn(mut self, f: impl FnOnce(&mut Engine)) -> Self {
        f(&mut self.engine);
        self
    }

    pub fn build(self) -> Host {
        Host {
            engine: Arc::new(self.engine),
            limits: self.limits,
            cache: AstCache::default(),
        }
    }
}

pub const AST_CACHE_BOUND: usize = 1024;

#[derive(Default)]
struct AstCache(Mutex<HashMap<String, Arc<AST>>>);

pub struct Host {
    engine: Arc<Engine>,
    limits: Limits,
    cache: AstCache,
}

impl Host {
    pub fn builder(limits: Limits) -> HostBuilder {
        let mut engine = Engine::new();
        limits.apply(&mut engine);
        crate::resolver::install(&mut engine);
        HostBuilder { limits, engine }
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Compile through the bounded cache. `key` must encode the compile mode
    /// as well as the source (`"e:<src>"` / `"s:<src>"`) so an expression and a
    /// script with identical text never share an AST.
    pub fn ast(
        &self,
        key: &str,
        src: &str,
        compile: impl FnOnce(&Engine) -> std::result::Result<AST, rhai::ParseError>,
    ) -> Result<Arc<AST>> {
        if let Some(ast) = self.cache.0.lock().unwrap().get(key) {
            return Ok(ast.clone());
        }
        let ast = self.ast_uncached(src, compile)?;
        let mut map = self.cache.0.lock().unwrap();
        if map.len() >= AST_CACHE_BOUND {
            map.clear();
        }
        map.insert(key.to_string(), ast.clone());
        Ok(ast)
    }

    /// Compile without touching the cache (one-off scripts).
    pub fn ast_uncached(
        &self,
        src: &str,
        compile: impl FnOnce(&Engine) -> std::result::Result<AST, rhai::ParseError>,
    ) -> Result<Arc<AST>> {
        compile(&self.engine)
            .map(Arc::new)
            .map_err(|e| RhaiError::from_parse(src, e))
    }

    #[cfg(test)]
    pub(crate) fn cache_len(&self) -> usize {
        self.cache.0.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocab_fn_registers_callable_functions() {
        let host = Host::builder(Limits::Ui)
            .vocab_fn(|e| {
                e.register_fn("double", |x: i64| x * 2);
            })
            .build();
        let ast = host
            .ast("e:double(21)", "double(21)", |e| {
                e.compile_expression("double(21)")
            })
            .unwrap();
        let v: i64 = host.engine().eval_ast(&ast).unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn same_source_compiles_once() {
        let host = Host::builder(Limits::Ui).build();
        let a = host
            .ast("e:1+1", "1+1", |e| e.compile_expression("1+1"))
            .unwrap();
        let b = host
            .ast("e:1+1", "1+1", |e| e.compile_expression("1+1"))
            .unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(host.cache_len(), 1);
    }

    #[test]
    fn overflow_clears_and_keeps_serving() {
        let host = Host::builder(Limits::Ui).build();
        for i in 0..AST_CACHE_BOUND {
            let src = format!("{i}");
            host.ast(&format!("e:{src}"), &src, |e| e.compile_expression(&src))
                .unwrap();
        }
        assert_eq!(host.cache_len(), AST_CACHE_BOUND);
        host.ast("e:overflow", "1", |e| e.compile_expression("1"))
            .unwrap();
        assert_eq!(host.cache_len(), 1);
        let v: i64 = host
            .engine()
            .eval_ast(&host.ast("e:2", "2", |e| e.compile_expression("2")).unwrap())
            .unwrap();
        assert_eq!(v, 2);
    }

    #[test]
    fn syntax_error_is_located() {
        let host = Host::builder(Limits::Ui).build();
        let err = host
            .ast("e:1 +", "1 +", |e| e.compile_expression("1 +"))
            .unwrap_err();
        assert!(matches!(err, RhaiError::Syntax(_)));
    }
}
