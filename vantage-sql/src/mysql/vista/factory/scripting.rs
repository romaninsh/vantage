//! The Rhai host shared by `modify:` scripts and scripted references: the
//! shell's MySQL vocabulary first, then vantage-vista's describe-only data
//! verbs, with `table(name)` building the named vista through the factory.

use vantage_core::Result;
use vantage_rhai::{Host, Limits};
use vantage_vista::Vista;
use vantage_vista::rhai::{DataVocab, Terminals, eval_modify_script, script_tables};

use super::MysqlVistaFactory;
use crate::mysql::vista::source::register_extensions;

impl MysqlVistaFactory {
    /// A host for scripts evaluated against MySQL vistas: the shell's
    /// vocabulary (its free functions under `sql::`) and the data verbs,
    /// where `table(name)` builds a table handle from a resolved spec. A SQL
    /// table needs its declared columns, so there is no bare-table fallback.
    pub(crate) fn script_host(&self) -> Host {
        let tables = script_tables(self.clone(), self.resolver.clone(), None);
        let data = DataVocab::new(Terminals::Describe).with_resolver(tables);
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
}
