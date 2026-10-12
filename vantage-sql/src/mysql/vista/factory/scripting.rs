//! The Rhai host shared by `modify:` scripts and scripted references: the
//! shell's MySQL vocabulary first, then vantage-vista's describe-only data
//! verbs, with `table(name)` building the named vista through the factory.

use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_rhai::{Host, Limits};
use vantage_vista::{DataVocab, TargetResolver, Vista, VistaFactory};

use super::MysqlVistaFactory;
use crate::mysql::vista::source::register_extensions;
use crate::mysql::vista::spec::MysqlVistaSpec;

impl MysqlVistaFactory {
    /// A host for scripts evaluated against MySQL vistas: the shell's
    /// vocabulary, then the data vocab. The data vocab goes second, so
    /// `table(name)` is a table handle rather than the SQL vocabulary's
    /// `table` alias for `ident`.
    pub(crate) fn script_host(&self) -> Host {
        Host::builder(Limits::background())
            .vocab_fn(register_extensions)
            .vocab(DataVocab::describe(Some(self.target_resolver())))
            .build()
    }

    /// Run a `mysql: { modify }` script against a built vista, exposed as
    /// `self`.
    pub(super) fn apply_modify(&self, vista: Vista, code: &str) -> Result<Vista> {
        vantage_vista::eval_modify_script(&self.script_host(), code, vista)
    }

    fn target_resolver(&self) -> TargetResolver {
        let factory = self.clone();
        Arc::new(move |name| factory.build_from_spec(factory.script_spec(name)?))
    }

    /// The spec a script's `table(name)` builds. A SQL table needs its declared
    /// columns, so a missing resolver or a name it doesn't know is an error
    /// rather than a bare table.
    fn script_spec(&self, name: &str) -> Result<MysqlVistaSpec> {
        let resolver = self
            .resolver
            .as_ref()
            .ok_or_else(|| error!("script table() requires a spec resolver on the factory"))?;
        resolver(name).ok_or_else(|| error!("script table(): unknown table", table = name))
    }
}
