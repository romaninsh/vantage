//! [`DataVocab`]: the one data vocabulary a host registers, configured by
//! which terminal verbs it allows.

use std::sync::Arc;

use vantage_core::Result;
use vantage_rhai::Vocab;
use vantage_rhai::rhai::Engine;

use super::narrow::register_narrowing;
use super::read::register_reads;
use super::record::register_record;
use super::write::register_writes;
use crate::vista::Vista;

/// Resolve a table name to a fresh, unconditioned [`Vista`]. The host owns
/// the by-name catalog; vantage-vista only calls it.
pub type TargetResolver = Arc<dyn Fn(&str) -> Result<Vista> + Send + Sync>;

/// Whether a [`Terminals::ReadWrite`] host may write.
#[derive(Clone)]
pub enum Writes {
    Allowed,
    /// Every write verb exists but throws this message.
    Denied(String),
}

/// Which terminal verbs a host registers on top of the narrowing verbs.
#[derive(Clone)]
pub enum Terminals {
    /// Narrowing only. The script's result is a handle the host reads.
    Describe,
    /// Read verbs; `limit` caps every `list()`.
    Read { limit: Option<usize> },
    /// Read and write verbs.
    ReadWrite {
        limit: Option<usize>,
        writes: Writes,
    },
}

/// The table handle vocabulary: `table(name)` (when `resolver` is set), the
/// narrowing verbs, and the terminals `terminals` allows.
///
/// Register it after any backend vocabulary
/// ([`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions)),
/// so its `table(name)` wins over a backend's own `table` function.
#[derive(Clone)]
pub struct DataVocab {
    pub resolver: Option<TargetResolver>,
    pub terminals: Terminals,
}

impl DataVocab {
    /// Narrowing only ([`Terminals::Describe`]).
    pub fn describe(resolver: Option<TargetResolver>) -> Self {
        Self {
            resolver,
            terminals: Terminals::Describe,
        }
    }

    /// Read verbs, `limit` capping every `list()` ([`Terminals::Read`]).
    pub fn read(resolver: Option<TargetResolver>, limit: Option<usize>) -> Self {
        Self {
            resolver,
            terminals: Terminals::Read { limit },
        }
    }

    /// Read and write verbs, writes allowed, `list()` uncapped.
    pub fn read_write(resolver: Option<TargetResolver>) -> Self {
        Self::read_write_with(resolver, None, Writes::Allowed)
    }

    /// Read and write verbs with an explicit `limit` and [`Writes`].
    pub fn read_write_with(
        resolver: Option<TargetResolver>,
        limit: Option<usize>,
        writes: Writes,
    ) -> Self {
        Self {
            resolver,
            terminals: Terminals::ReadWrite { limit, writes },
        }
    }
}

impl Vocab for DataVocab {
    fn register(&self, engine: &mut Engine) {
        register_narrowing(engine, self.resolver.clone());
        match &self.terminals {
            Terminals::Describe => {}
            Terminals::Read { limit } => {
                register_reads(engine, self.resolver.clone(), *limit);
                register_record(
                    engine,
                    self.resolver.clone(),
                    Writes::Denied("writes aren't available here".to_string()),
                );
            }
            Terminals::ReadWrite { limit, writes } => {
                register_reads(engine, self.resolver.clone(), *limit);
                register_writes(engine, self.resolver.clone(), writes.clone());
                register_record(engine, self.resolver.clone(), writes.clone());
            }
        }
    }
}
