//! [`DataVocab`]: the one data vocabulary a host registers, configured by
//! which terminal verbs it allows.

use std::sync::Arc;

use vantage_rhai::Vocab;
use vantage_rhai::rhai::Engine;

use super::narrow::register_narrowing;
use super::read::register_reads;
use super::record::register_record;
use super::write::register_writes;
use crate::VistaResolver;

/// Whether a [`Terminals::ReadWrite`] host may write.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Writes {
    Allowed,
    /// Every write verb exists but throws this message.
    Denied(String),
}

/// Which terminal verbs a host registers on top of the narrowing verbs.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum Terminals {
    /// Narrowing only. The script's result is a handle the host reads.
    Describe,
    /// Read and write verbs; `limit` caps every `list()`. A read-only host
    /// passes [`Writes::Denied`], so every write throws its message.
    ReadWrite {
        limit: Option<usize>,
        writes: Writes,
    },
}

/// The table handle vocabulary: the narrowing verbs, the terminals
/// `terminals` allows, and `table(name)` once a resolver is set.
///
/// ```
/// # use std::sync::Arc;
/// # use vantage_vista::VistaResolver;
/// use vantage_vista::rhai::{DataVocab, Terminals, Writes};
///
/// fn agent_vocab(tables: Arc<dyn VistaResolver>) -> DataVocab {
///     let terminals = Terminals::ReadWrite { limit: Some(50), writes: Writes::Allowed };
///     DataVocab::new(terminals).with_resolver(tables)
/// }
/// ```
///
/// Backend vocabulary
/// ([`TableShell::register_rhai_extensions`](crate::TableShell::register_rhai_extensions))
/// shares no names with it, so the two register in either order.
#[derive(Clone)]
pub struct DataVocab {
    resolver: Option<Arc<dyn VistaResolver>>,
    terminals: Terminals,
}

impl DataVocab {
    /// The vocabulary with `terminals` and no `table(name)`: scripts reach
    /// data only through handles the host passes in.
    pub fn new(terminals: Terminals) -> Self {
        Self {
            resolver: None,
            terminals,
        }
    }

    /// Register `table(name)`, resolving each name through `resolver`.
    pub fn with_resolver(mut self, resolver: Arc<dyn VistaResolver>) -> Self {
        self.resolver = Some(resolver);
        self
    }
}

impl Vocab for DataVocab {
    fn register(&self, engine: &mut Engine) {
        register_narrowing(engine, self.resolver.clone());
        match &self.terminals {
            Terminals::Describe => {}
            Terminals::ReadWrite { limit, writes } => {
                register_reads(engine, self.resolver.clone(), *limit);
                register_writes(engine, self.resolver.clone(), writes.clone());
                register_record(engine, self.resolver.clone(), writes.clone());
            }
        }
    }
}
