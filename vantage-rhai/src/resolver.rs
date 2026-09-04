//! Lazy variable resolution through a `Resolver`, with namespace descent and
//! read recording for discovery.
//!
//! rhai consults `on_var` BEFORE it searches the scope, so the hook yields
//! (`Ok(None)`) for any name the scope already holds: pushed variables shadow
//! the resolver.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use rhai::{Dynamic, Engine, EvalAltResult, Position};

pub enum Lookup {
    Leaf(Dynamic),
    Namespace,
    Unknown,
}

pub trait Resolver: Send + Sync {
    /// Resolve a dotted path. `Namespace` means "keep descending".
    fn resolve(&self, path: &str) -> Lookup;
}

pub(crate) const ENV_KEY: &str = "__vantage_env";

/// The per-evaluation resolver, smuggled into the scope as a constant so a
/// shared engine can serve a different resolver on every call.
#[derive(Clone)]
pub(crate) struct EnvHandle(pub Arc<dyn Resolver>);

/// Wraps a resolver and records every leaf path it resolves.
pub(crate) struct RecordingResolver {
    inner: Arc<dyn Resolver>,
    reads: Mutex<BTreeSet<String>>,
}

impl RecordingResolver {
    pub fn new(inner: Arc<dyn Resolver>) -> Self {
        RecordingResolver {
            inner,
            reads: Mutex::new(BTreeSet::new()),
        }
    }

    pub fn take_reads(&self) -> BTreeSet<String> {
        std::mem::take(&mut *self.reads.lock().unwrap())
    }
}

impl Resolver for RecordingResolver {
    fn resolve(&self, path: &str) -> Lookup {
        let out = self.inner.resolve(path);
        if matches!(out, Lookup::Leaf(_)) {
            self.reads.lock().unwrap().insert(path.to_string());
        }
        out
    }
}

/// A namespace prefix. Indexing (or `.prop`, which rhai routes to the indexer
/// when no getter exists) descends and resolves leaves through the resolver.
#[derive(Clone)]
struct NamespaceProxy {
    resolver: Arc<dyn Resolver>,
    prefix: String,
}

fn lookup(resolver: &Arc<dyn Resolver>, path: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    match resolver.resolve(path) {
        Lookup::Leaf(v) => Ok(v),
        Lookup::Namespace => Ok(Dynamic::from(NamespaceProxy {
            resolver: resolver.clone(),
            prefix: path.to_string(),
        })),
        Lookup::Unknown => Err(Box::new(EvalAltResult::ErrorVariableNotFound(
            path.to_string(),
            Position::NONE,
        ))),
    }
}

fn proxy_index(proxy: &mut NamespaceProxy, key: &str) -> Result<Dynamic, Box<EvalAltResult>> {
    let path = format!("{}.{}", proxy.prefix, key);
    lookup(&proxy.resolver, &path)
}

pub(crate) fn install(engine: &mut Engine) {
    engine.register_type_with_name::<NamespaceProxy>("Namespace");
    engine.register_indexer_get(proxy_index);
    // `on_var` is rhai's variable-resolver hook (flagged "volatile", not truly
    // deprecated — see the rhai docs).
    #[allow(deprecated)]
    engine.on_var(|name, _index, ctx| {
        if name == ENV_KEY || ctx.scope().contains(name) {
            return Ok(None);
        }
        let Some(handle) = ctx.scope().get_value::<EnvHandle>(ENV_KEY) else {
            return Ok(None);
        };
        match handle.0.resolve(name) {
            Lookup::Unknown => Ok(None), // let rhai report it
            _ => lookup(&handle.0, name).map(Some),
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A resolver over a flat `path → value` map; any strict prefix of a known
    /// path is a namespace.
    pub(crate) struct MapResolver(pub HashMap<String, Dynamic>);

    impl Resolver for MapResolver {
        fn resolve(&self, path: &str) -> Lookup {
            if let Some(v) = self.0.get(path) {
                return Lookup::Leaf(v.clone());
            }
            let prefix = format!("{path}.");
            if self.0.keys().any(|k| k.starts_with(&prefix)) {
                Lookup::Namespace
            } else {
                Lookup::Unknown
            }
        }
    }

    fn demo() -> Arc<dyn Resolver> {
        let mut m = HashMap::new();
        m.insert("app.toolbar.currency".to_string(), Dynamic::from("USD"));
        m.insert("args.id".to_string(), Dynamic::from(42_i64));
        Arc::new(MapResolver(m))
    }

    fn engine_with(resolver: Arc<dyn Resolver>) -> (Engine, rhai::Scope<'static>) {
        let mut engine = Engine::new();
        install(&mut engine);
        let mut scope = rhai::Scope::new();
        scope.push_constant(ENV_KEY, EnvHandle(resolver));
        (engine, scope)
    }

    #[test]
    fn dotted_reads_descend_namespaces() {
        let (engine, mut scope) = engine_with(demo());
        let v: String = engine
            .eval_with_scope(&mut scope, "app.toolbar.currency")
            .unwrap();
        assert_eq!(v, "USD");
        let n: i64 = engine.eval_with_scope(&mut scope, "args.id + 1").unwrap();
        assert_eq!(n, 43);
    }

    #[test]
    fn pushed_variables_shadow_the_resolver() {
        let (engine, mut scope) = engine_with(demo());
        scope.push("args", 7_i64);
        let n: i64 = engine.eval_with_scope(&mut scope, "args").unwrap();
        assert_eq!(n, 7);
    }

    #[test]
    fn unknown_leaf_is_variable_not_found() {
        let (engine, mut scope) = engine_with(demo());
        let err = engine
            .eval_with_scope::<Dynamic>(&mut scope, "app.toolbar.nope")
            .unwrap_err();
        assert!(
            matches!(*err, EvalAltResult::ErrorVariableNotFound(ref p, _) if p == "app.toolbar.nope")
        );
        let err = engine
            .eval_with_scope::<Dynamic>(&mut scope, "nothing")
            .unwrap_err();
        assert!(matches!(*err, EvalAltResult::ErrorVariableNotFound(..)));
    }

    #[test]
    fn recording_resolver_records_leaf_paths_only() {
        let rec = Arc::new(RecordingResolver::new(demo()));
        let as_dyn: Arc<dyn Resolver> = rec.clone();
        let (engine, mut scope) = engine_with(as_dyn);
        let _ = engine
            .eval_with_scope::<Dynamic>(&mut scope, "app.toolbar.currency + args.id.to_string()")
            .unwrap();
        let reads = rec.take_reads();
        assert_eq!(
            reads.iter().collect::<Vec<_>>(),
            vec!["app.toolbar.currency", "args.id"]
        );
    }
}
