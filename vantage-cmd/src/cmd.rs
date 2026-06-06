//! The [`Cmd`] datasource — a locked command, declared env, and a
//! registry of per-table Rhai scripts.

use std::sync::Arc;

use indexmap::IndexMap;
use vantage_core::{Result, error};

/// Per-table configuration registered on a [`Cmd`]: the Rhai script that
/// builds the argv and parses the output, plus optional command / env
/// overrides that win over the datasource-level defaults.
#[derive(Clone, Debug)]
pub struct CmdSpec {
    pub script: Arc<str>,
    pub command: Option<String>,
    pub env: IndexMap<String, String>,
}

impl CmdSpec {
    pub fn new(script: impl Into<Arc<str>>) -> Self {
        Self {
            script: script.into(),
            command: None,
            env: IndexMap::new(),
        }
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
#[derive(Clone, Debug)]
pub struct Cmd {
    command: Arc<str>,
    env: Arc<IndexMap<String, String>>,
    pass_path: bool,
    scripts: Arc<IndexMap<String, CmdSpec>>,
}

impl Cmd {
    /// Build a datasource locked to `command` (e.g. `"aws"`).
    pub fn new(command: impl Into<Arc<str>>) -> Self {
        Self {
            command: command.into(),
            env: Arc::new(IndexMap::new()),
            pass_path: true,
            scripts: Arc::new(IndexMap::new()),
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

    /// A Vista factory bound to this datasource.
    pub fn vista_factory(&self) -> crate::vista::factory::CmdVistaFactory {
        crate::vista::factory::CmdVistaFactory::new(self.clone())
    }
}
