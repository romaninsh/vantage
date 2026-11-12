//! `import "name"` from one folder of `.rhai` files (a project's `lib/`).
//!
//! Names are root-relative (`"plans"`, `"checks/kind"`), never paths: no `..`,
//! no absolute paths, no extension. A module is compiled on the importing
//! host's engine and evaluated in a fresh scope, so neither its top level nor
//! its functions see the importing body's variables or resolver; a helper
//! works only on its arguments. Loaded modules stay cached until
//! [`ModuleDir::changed`] names their file.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rhai::module_resolvers::DummyModuleResolver;
use rhai::{
    AST, ASTNode, Engine, EvalAltResult, Expr, Module, ModuleResolver, NativeCallContext, Position,
    Scope, Stmt,
};

/// The AST source of every module, which is how [`ModuleDir::is_helper_call`]
/// tells a helper's call from a body's (bodies carry no source).
const SOURCE_PREFIX: &str = "lib:";

// rhai's `SharedModule` is private; with the `sync` feature it is this.
type SharedModule = Arc<Module>;
type ReloadHook = Arc<dyn Fn(&BTreeSet<String>) + Send + Sync>;

struct Loaded {
    module: SharedModule,
    imports: BTreeSet<String>,
}

struct Inner {
    root: PathBuf,
    cache: Mutex<HashMap<String, Loaded>>,
    hooks: Mutex<Vec<ReloadHook>>,
    /// Bumped by every [`ModuleDir::changed`], under the cache lock. A load
    /// that saw another value before reading its file may hold old code, so
    /// it isn't cached.
    generation: AtomicU64,
}

/// A module folder shared by every host that imports from it, and the handle
/// a file watcher reports edits to. Cloning shares the cache and the hooks.
#[derive(Clone)]
pub struct ModuleDir {
    inner: Arc<Inner>,
}

impl ModuleDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ModuleDir {
            inner: Arc::new(Inner {
                root: root.into(),
                cache: Mutex::default(),
                hooks: Mutex::default(),
                generation: AtomicU64::new(0),
            }),
        }
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// Called after [`changed`](Self::changed) with the module names whose
    /// code may now differ; a body whose [`imports`](crate::Compiled::imports)
    /// meets the set should re-run.
    pub fn on_reload(&self, hook: impl Fn(&BTreeSet<String>) + Send + Sync + 'static) {
        self.inner.hooks.lock().unwrap().push(Arc::new(hook));
    }

    /// A file under the root was written, created or removed. Drops that
    /// module and every cached module importing it (transitively), notifies
    /// the hooks and returns the names. A path that is not a `.rhai` module
    /// under the root changes nothing and returns an empty set.
    pub fn changed(&self, file: &Path) -> BTreeSet<String> {
        let Some(name) = self.name_of(file) else {
            return BTreeSet::new();
        };
        let affected = {
            let mut cache = self.inner.cache.lock().unwrap();
            self.inner.generation.fetch_add(1, Ordering::SeqCst);
            let mut affected = BTreeSet::from([name]);
            loop {
                let more: Vec<String> = cache
                    .iter()
                    .filter(|(n, l)| !affected.contains(*n) && !l.imports.is_disjoint(&affected))
                    .map(|(n, _)| n.clone())
                    .collect();
                if more.is_empty() {
                    break;
                }
                affected.extend(more);
            }
            cache.retain(|n, _| !affected.contains(n));
            affected
        };
        let hooks = self.inner.hooks.lock().unwrap().clone();
        for hook in hooks {
            hook(&affected);
        }
        affected
    }

    fn name_of(&self, file: &Path) -> Option<String> {
        let root = &self.inner.root;
        let rel = match file.strip_prefix(root) {
            Ok(rel) => rel.to_path_buf(),
            // A watcher may report canonical paths (macOS `/private/var/…`);
            // the parent still exists when the file was just removed.
            Err(_) => {
                let parent = file.parent()?.canonicalize().ok()?;
                let rel = parent.strip_prefix(root.canonicalize().ok()?).ok()?;
                rel.join(file.file_name()?)
            }
        };
        if rel.extension()? != "rhai" {
            return None;
        }
        let name = rel
            .with_extension("")
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect::<Option<Vec<_>>>()?
            .join("/");
        valid_name(&name).then_some(name)
    }

    /// True when a native function was called from a module's code rather
    /// than from a body. A verb that reaches beyond its arguments (`table()`)
    /// refuses then, so permissions stay with the body that runs.
    pub fn is_helper_call(ctx: &NativeCallContext) -> bool {
        ctx.call_source()
            .is_some_and(|s| s.starts_with(SOURCE_PREFIX))
    }

    pub(crate) fn install(&self, engine: &mut Engine) {
        engine.set_module_resolver(DirResolver(self.inner.clone()));
    }
}

/// Without a [`ModuleDir`], `import` resolves nothing. rhai's own default
/// reads files relative to the process's working directory.
pub(crate) fn install_none(engine: &mut Engine) {
    engine.set_module_resolver(DummyModuleResolver::new());
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('/').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// String-constant `import` paths anywhere in `ast`, function bodies included.
pub(crate) fn imports_of(ast: &AST, out: &mut BTreeSet<String>) {
    ast.walk(&mut |path: &[ASTNode]| {
        if let Some(ASTNode::Stmt(Stmt::Import(x, _))) = path.last()
            && let Expr::StringConstant(name, _) = &x.0
        {
            out.insert(name.to_string());
        }
        true
    });
}

thread_local! {
    /// Modules being loaded on this thread, outermost first: an import cycle
    /// would otherwise recurse until the stack overflows.
    static LOADING: RefCell<Vec<(usize, String)>> = const { RefCell::new(Vec::new()) };
}

struct LoadingGuard;

impl Drop for LoadingGuard {
    fn drop(&mut self) {
        LOADING.with(|l| l.borrow_mut().pop());
    }
}

struct DirResolver(Arc<Inner>);

fn runtime(msg: String, pos: Position) -> Box<EvalAltResult> {
    Box::new(EvalAltResult::ErrorRuntime(msg.into(), pos))
}

impl ModuleResolver for DirResolver {
    fn resolve(
        &self,
        engine: &Engine,
        _source: Option<&str>,
        name: &str,
        pos: Position,
    ) -> Result<SharedModule, Box<EvalAltResult>> {
        if !valid_name(name) {
            return Err(runtime(
                format!(
                    "import {name:?}: name a lib/ module (letters, digits, `_`, `-`; \
                     `/` between folders; no extension)"
                ),
                pos,
            ));
        }
        if let Some(loaded) = self.0.cache.lock().unwrap().get(name) {
            return Ok(loaded.module.clone());
        }

        let key = (Arc::as_ptr(&self.0) as usize, name.to_string());
        let cycle = LOADING.with(|l| {
            let l = l.borrow();
            l.iter()
                .position(|k| *k == key)
                .map(|at| l[at..].iter().map(|(_, n)| n.clone()).collect::<Vec<_>>())
        });
        if let Some(chain) = cycle {
            return Err(runtime(
                format!("import cycle: {} -> {name}", chain.join(" -> ")),
                pos,
            ));
        }
        LOADING.with(|l| l.borrow_mut().push(key));
        let _guard = LoadingGuard;

        let generation = self.0.generation.load(Ordering::SeqCst);
        let file = self.0.root.join(format!("{name}.rhai"));
        let src = std::fs::read_to_string(&file).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                Box::new(EvalAltResult::ErrorModuleNotFound(name.to_string(), pos))
            }
            _ => runtime(format!("import {name:?}: {}: {e}", file.display()), pos),
        })?;
        let in_module = |err: Box<EvalAltResult>| {
            Box::new(EvalAltResult::ErrorInModule(name.to_string(), err, pos))
        };
        let mut ast = engine.compile(&src).map_err(|e| in_module(e.into()))?;
        ast.set_source(format!("{SOURCE_PREFIX}{name}"));
        let mut imports = BTreeSet::new();
        imports_of(&ast, &mut imports);
        let module: SharedModule = Module::eval_ast_as_new(Scope::new(), &ast, engine)
            .map_err(in_module)?
            .into();

        let mut cache = self.0.cache.lock().unwrap();
        if self.0.generation.load(Ordering::SeqCst) == generation {
            cache.insert(
                name.to_string(),
                Loaded {
                    module: module.clone(),
                    imports,
                },
            );
        }
        Ok(module)
    }
}
