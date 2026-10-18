//! The build injector: a hook a vista factory runs on every table it builds
//! from a spec — the root, each reference target and each `base:` table —
//! after the table is built and before it is wrapped. It may add conditions
//! and register hooks, or fail the build.
//!
//! Each backend's spec builder threads a [`BuildEnv`] through its builds and
//! ends with [`BuildEnv::inject`]; `Table::with_spec_references` hands each
//! reference target a fresh environment of kind [`BuildKind::Reference`].

use std::fmt;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_types::EmptyEntity;
use vantage_vista::SpecResolver;

use crate::table::Table;
use crate::traits::table_source::TableSource;

/// Why a table is being built.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BuildKind {
    /// A table built for its own sake: a factory's `build_from_spec`, or a
    /// table an injector asked for through [`InjectCx::build`].
    Root,
    /// The target of `relation` on the spec named `from`, built when the
    /// relation is traversed.
    Reference { from: String, relation: String },
    /// The `base:` table the spec named `derived` reads from. Only the
    /// conditions an injector adds here take effect: the derived table is a
    /// fresh table over the base's `select()`, so hooks registered on a
    /// `Base` build never fire. Register hooks on the derived build instead.
    Base { derived: String },
}

/// What an injector receives besides the table: the spec key being built,
/// why, and a builder for other tables of the same backend.
#[non_exhaustive]
pub struct InjectCx<'a, T: TableSource> {
    pub key: &'a str,
    pub kind: BuildKind,
    /// Build another table by spec key, through the same factory and the
    /// same injector (its kind is [`BuildKind::Root`]). Errors when the
    /// factory has no resolver, the key has no spec, or the build would
    /// re-enter a key already being built (a cycle).
    pub build: &'a dyn Fn(&str) -> Result<Table<T, EmptyEntity>>,
}

/// A build injector. Typed per backend, since each backend's table is.
pub type TableInjector<T> =
    Arc<dyn Fn(&InjectCx<'_, T>, &mut Table<T, EmptyEntity>) -> Result<()> + Send + Sync>;

/// The spec keys being built, outermost first. Nested builds (an injector
/// building a parent, a `base:` chain) extend it; re-entering a key is a cycle.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildTrail {
    keys: Vec<String>,
}

impl BuildTrail {
    /// The trail with `key` appended; an error naming the path when `key` is
    /// already on it.
    pub fn enter(&self, key: &str) -> Result<BuildTrail> {
        if self.keys.iter().any(|k| k == key) {
            let path = self
                .keys
                .iter()
                .map(String::as_str)
                .chain([key])
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(error!("table build cycle", path = path));
        }
        let mut keys = self.keys.clone();
        keys.push(key.to_string());
        Ok(BuildTrail { keys })
    }

    pub fn keys(&self) -> &[String] {
        &self.keys
    }
}

/// Builds a table from spec `S` over data source `T`, carrying the build
/// environment into nested builds.
pub type SpecTableBuilder<S, T> = fn(&S, T, &BuildEnv<S, T>) -> Result<Table<T, EmptyEntity>>;

/// What a spec build carries besides the spec and the data source: the
/// resolver for references and `base:`, the injector, the trail of keys
/// being built, and the kind of this build.
pub struct BuildEnv<S, T: TableSource> {
    pub resolver: Option<Arc<dyn SpecResolver<S>>>,
    pub injector: Option<TableInjector<T>>,
    pub trail: BuildTrail,
    pub kind: BuildKind,
}

impl<S, T: TableSource> Clone for BuildEnv<S, T> {
    fn clone(&self) -> Self {
        Self {
            resolver: self.resolver.clone(),
            injector: self.injector.clone(),
            trail: self.trail.clone(),
            kind: self.kind.clone(),
        }
    }
}

impl<S, T: TableSource> fmt::Debug for BuildEnv<S, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BuildEnv")
            .field("resolver", &self.resolver.is_some())
            .field("injector", &self.injector.is_some())
            .field("trail", &self.trail)
            .field("kind", &self.kind)
            .finish()
    }
}

impl<S: 'static, T: TableSource> BuildEnv<S, T> {
    /// The environment of a root build of `key`.
    pub fn root(
        key: &str,
        resolver: Option<Arc<dyn SpecResolver<S>>>,
        injector: Option<TableInjector<T>>,
    ) -> Self {
        Self {
            resolver,
            injector,
            trail: BuildTrail {
                keys: vec![key.to_string()],
            },
            kind: BuildKind::Root,
        }
    }

    /// The environment for building `key` from inside this build, as `kind`.
    /// Errors when `key` is already being built.
    pub fn nested(&self, key: &str, kind: BuildKind) -> Result<Self> {
        Ok(Self {
            resolver: self.resolver.clone(),
            injector: self.injector.clone(),
            trail: self.trail.enter(key)?,
            kind,
        })
    }

    /// Run the injector, if any, on `table`, the result of building `key`.
    /// `build` is the backend's spec builder, used for the tables the
    /// injector asks for.
    pub fn inject(
        &self,
        key: &str,
        table: &mut Table<T, EmptyEntity>,
        build: SpecTableBuilder<S, T>,
    ) -> Result<()> {
        let Some(injector) = &self.injector else {
            return Ok(());
        };
        let db = table.data_source().clone();
        let nested = |other: &str| -> Result<Table<T, EmptyEntity>> {
            let env = self.nested(other, BuildKind::Root)?;
            let resolver = self.resolver.as_ref().ok_or_else(|| {
                error!(
                    "the injector's table builder needs a spec resolver",
                    key = other
                )
            })?;
            let spec = resolver
                .resolve(other)
                .ok_or_else(|| error!("the injector's table builder found no spec", key = other))?;
            build(&spec, db.clone(), &env)
        };
        let cx = InjectCx {
            key,
            kind: self.kind.clone(),
            build: &nested,
        };
        injector(&cx, table)
    }
}
