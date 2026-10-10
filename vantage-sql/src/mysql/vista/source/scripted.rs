//! Rhai on the shell: the `with_condition` verb and scripted reference
//! traversal.
//!
//! MySQL has no current-record anchor of its own, so a reference build-script
//! sees the parent row twice: as `row`, like every backend, and as `me`.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_rhai::Env;
use vantage_rhai::rhai::{Engine, EvalAltResult};
use vantage_types::{Entity, Record};
use vantage_vista::{Handle, TableShell, Vista};

use super::MysqlTableShell;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::rhai_source::register_mysql_onto;
use crate::rhai_engine::RhaiExpr;

/// The MySQL vocabulary plus `self.with_condition(<expr>)` on the vista
/// handle, which routes the expression through `add_raw_condition`.
pub(super) fn register_extensions(engine: &mut Engine) {
    register_mysql_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut Handle,
         cond: RhaiExpr<AnyMysqlType>|
         -> std::result::Result<Handle, Box<EvalAltResult>> {
            h.with_base_vista("with_condition", |vista| vista.add_raw_condition(cond.0))
                .map_err(|e| e.to_string().into())
        },
    );
}

impl<E> MysqlTableShell<E>
where
    E: Entity<AnyMysqlType> + 'static,
{
    /// Build a reference's traversal target by evaluating its `build_script`;
    /// `table(name)` builds the named vista through this shell's factory.
    pub(super) fn get_ref_via_script(
        &self,
        script: &str,
        row: &Record<CborValue>,
    ) -> Result<Vista> {
        let host = self.factory().script_host(self);
        let env = self
            .rhai_env(Env::new())
            .var("me", vantage_vista::record_to_dynamic(row));
        vantage_vista::eval_ref_script(&host, script, env, row)
    }
}
