//! The [`Cmd`] datasource — a locked command, declared env, and a
//! registry of per-table Rhai scripts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use vantage_core::{Result, error};

use crate::rhai_engine::CompiledScript;

/// Per-table configuration registered on a [`Cmd`]: the Rhai script that
/// builds the argv and parses the output, plus optional command / env
/// overrides that win over the datasource-level defaults.
#[derive(Clone, Debug)]
pub struct CmdSpec {
    pub script: Arc<str>,
    /// Optional per-row detail script. When set, the table loads in two
    /// passes: `script` lists id stubs, and `detail` hydrates one record at
    /// a time (with `id` in scope) via `get_value`. Both run the same locked
    /// command — only the argv the script builds differs (e.g. gh's `runs`
    /// vs `stats`). When `None`, the table is single-pass as before.
    pub detail: Option<Arc<str>>,
    pub command: Option<String>,
    pub env: IndexMap<String, String>,
}

impl CmdSpec {
    pub fn new(script: impl Into<Arc<str>>) -> Self {
        Self {
            script: script.into(),
            detail: None,
            command: None,
            env: IndexMap::new(),
        }
    }

    /// Register a per-row detail script (opt into two-pass loading).
    pub fn with_detail(mut self, detail: impl Into<Arc<str>>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Override the locked command for this table only.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Declare an env var for this table only (merged over, and winning
    /// against, the datasource-level env).
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }
}

/// A command-execution datasource.
///
/// Cheap to clone (everything is `Arc`-backed). The `command` and `env`
/// here are the locked defaults; individual tables can be registered with
/// their own [`CmdSpec`] overrides via [`Cmd::with_table`].
#[derive(Clone)]
pub struct Cmd {
    command: Arc<str>,
    env: Arc<IndexMap<String, String>>,
    pass_path: bool,
    base_dir: Option<Arc<Path>>,
    scripts: Arc<IndexMap<String, CmdSpec>>,
    /// Memoized compiled scripts, keyed by script name. Shared across
    /// clones so a per-row detail loop reuses one engine + AST. Built
    /// lazily on first use (see [`Cmd::compiled_script`]).
    compiled: Arc<Mutex<HashMap<String, Arc<CompiledScript>>>>,
    /// How many times each named script has actually been compiled —
    /// diagnostics / test instrumentation for the reuse guarantee.
    compile_counts: Arc<Mutex<HashMap<String, usize>>>,
}

impl std::fmt::Debug for Cmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cmd")
            .field("command", &self.command)
            .field("env", &self.env)
            .field("pass_path", &self.pass_path)
            .field("base_dir", &self.base_dir)
            .field("scripts", &self.scripts)
            .finish()
    }
}

impl Cmd {
    /// Build a datasource locked to `command` (e.g. `"aws"`).
    pub fn new(command: impl Into<Arc<str>>) -> Self {
        Self {
            command: command.into(),
            env: Arc::new(IndexMap::new()),
            pass_path: true,
            base_dir: None,
            scripts: Arc::new(IndexMap::new()),
            compiled: Arc::new(Mutex::new(HashMap::new())),
            compile_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Declare a datasource-level env var, passed to every table's child
    /// process unless a table overrides it.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        Arc::make_mut(&mut self.env).insert(key.into(), value.into());
        self
    }

    /// Whether to forward `PATH`/`HOME` from the current process so the
    /// command can be located. Defaults to `true`; set `false` to require
    /// an absolute command path and a fully-declared environment.
    pub fn with_pass_path(mut self, pass_path: bool) -> Self {
        self.pass_path = pass_path;
        self
    }

    /// Set the base directory used to resolve a relative `command` *path*
    /// and as the child process's working directory.
    ///
    /// A `command` that contains a path separator but isn't absolute (e.g.
    /// `./scripts/gh-stats.py`) is resolved against this directory; bare
    /// names (e.g. `gh`) are left untouched for `PATH` lookup, and absolute
    /// paths pass through. When set, every table's child process also runs
    /// with this directory as its working directory, so a script can resolve
    /// sibling files relative to it.
    pub fn with_base_dir(mut self, base_dir: impl Into<PathBuf>) -> Self {
        self.base_dir = Some(Arc::from(base_dir.into()));
        self
    }

    /// Register a script under `name` with no overrides.
    pub fn with_script(self, name: impl Into<String>, script: impl Into<Arc<str>>) -> Self {
        self.with_table(name, CmdSpec::new(script))
    }

    /// Register a fully-specified [`CmdSpec`] under `name`.
    pub fn with_table(mut self, name: impl Into<String>, spec: CmdSpec) -> Self {
        Arc::make_mut(&mut self.scripts).insert(name.into(), spec);
        self
    }

    /// The locked default command.
    pub fn command(&self) -> &str {
        &self.command
    }

    pub(crate) fn pass_path(&self) -> bool {
        self.pass_path
    }

    pub(crate) fn base_dir(&self) -> Option<Arc<Path>> {
        self.base_dir.clone()
    }

    pub(crate) fn spec_for(&self, name: &str) -> Result<&CmdSpec> {
        self.scripts.get(name).ok_or_else(|| {
            error!(
                "no command script registered for table",
                table = name.to_string()
            )
        })
    }

    /// Effective command for a table: the spec override, else the locked default.
    pub(crate) fn effective_command(&self, spec: &CmdSpec) -> String {
        spec.command
            .clone()
            .unwrap_or_else(|| self.command.to_string())
    }

    /// Effective env for a table: datasource env, with the spec's env
    /// merged on top (spec wins on key clash).
    pub(crate) fn effective_env(&self, spec: &CmdSpec) -> IndexMap<String, String> {
        let mut env = (*self.env).clone();
        for (k, v) in &spec.env {
            env.insert(k.clone(), v.clone());
        }
        env
    }

    /// Get (building on first use) the compiled engine + AST for the named
    /// table's list script. Reused across calls.
    pub(crate) fn compiled_list_script(&self, name: &str) -> Result<Arc<CompiledScript>> {
        let spec = self.spec_for(name)?.clone();
        self.compiled_for(name.to_string(), &spec, &spec.script)
    }

    /// Get the compiled detail script for the named table, or `None` if the
    /// table has no detail script (single-pass). Reused across calls so a
    /// per-row detail loop pays the parse/registration cost once.
    pub(crate) fn compiled_detail_script(&self, name: &str) -> Result<Option<Arc<CompiledScript>>> {
        let spec = self.spec_for(name)?.clone();
        let Some(detail) = spec.detail.clone() else {
            return Ok(None);
        };
        Ok(Some(self.compiled_for(
            format!("{name}::detail"),
            &spec,
            &detail,
        )?))
    }

    /// Compile (once) and memoize a script under `cache_key`, using the
    /// spec's effective command + env. Shared across `Cmd` clones.
    fn compiled_for(
        &self,
        cache_key: String,
        spec: &CmdSpec,
        script: &str,
    ) -> Result<Arc<CompiledScript>> {
        let mut cache = self.compiled.lock().unwrap();
        if let Some(existing) = cache.get(&cache_key) {
            return Ok(existing.clone());
        }
        let command = self.effective_command(spec);
        let env = self.effective_env(spec);
        let compiled = Arc::new(CompiledScript::compile(
            command,
            env,
            self.pass_path(),
            self.base_dir(),
            script,
        )?);
        *self
            .compile_counts
            .lock()
            .unwrap()
            .entry(cache_key.clone())
            .or_insert(0) += 1;
        cache.insert(cache_key, compiled.clone());
        Ok(compiled)
    }

    /// True if the named table has a detail script (two-pass loading).
    pub(crate) fn has_detail_script(&self, name: &str) -> bool {
        self.spec_for(name)
            .map(|s| s.detail.is_some())
            .unwrap_or(false)
    }

    /// How many times the named script has been compiled. Reuse means this
    /// stays at 1 no matter how many reads run.
    pub fn compile_count(&self, name: &str) -> usize {
        self.compile_counts
            .lock()
            .unwrap()
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    /// A Vista factory bound to this datasource.
    pub fn vista_factory(&self) -> crate::vista::factory::CmdVistaFactory {
        crate::vista::factory::CmdVistaFactory::new(self.clone())
    }
}
