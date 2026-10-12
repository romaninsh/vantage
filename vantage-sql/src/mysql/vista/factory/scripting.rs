//! The Rhai host shared by `modify:` scripts and scripted references: the
//! shell's MySQL vocabulary first, then vantage-vista's describe-only data
//! verbs, with `table(name)` building the named vista through the factory.

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_rhai::{Host, Limits};
use vantage_vista::rhai::{DataVocab, Terminals, eval_modify_script};
use vantage_vista::{Vista, VistaFactory, VistaResolver};

use super::MysqlVistaFactory;
use crate::mysql::vista::source::register_extensions;
use crate::mysql::vista::spec::MysqlVistaSpec;

impl MysqlVistaFactory {
    /// A host for scripts evaluated against MySQL vistas: the shell's
    /// vocabulary (its free functions under `sql::`) and the data verbs,
    /// where `table(name)` builds a table handle.
    pub(crate) fn script_host(&self) -> Host {
        let data = DataVocab::new(Terminals::Describe).with_resolver(self.target_resolver());
        Host::builder(Limits::background())
            .vocab_fn(register_extensions)
            .vocab(data)
            .build()
    }

    /// Run a `mysql: { modify }` script against a built vista, exposed as
    /// `self`.
    pub(super) fn apply_modify(&self, vista: Vista, code: &str) -> Result<Vista> {
        eval_modify_script(&self.script_host(), code, vista)
    }

    fn target_resolver(&self) -> Arc<dyn VistaResolver> {
        let factory = self.clone();
        Arc::new(move |name: &str| factory.build_from_spec(factory.script_spec(name)?))
    }

    /// The spec a script's `table(name)` builds. A SQL table needs its declared
    /// columns, so a missing resolver is an error and a name it doesn't know
    /// is not-found, rather than a bare table.
    fn script_spec(&self, name: &str) -> Result<MysqlVistaSpec> {
        let resolver = self
            .resolver
            .as_ref()
            .ok_or_else(|| error!("script table() requires a spec resolver on the factory"))?;
        resolver(name)
            .ok_or_else(|| error!("script table(): unknown table", table = name).mark_not_found())
    }
}
