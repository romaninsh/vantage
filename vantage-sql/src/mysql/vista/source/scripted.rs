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
use vantage_vista::rhai::{Handle, eval_ref_script, record_to_dynamic};
use vantage_vista::{NativeCondition, Vista};

use super::MysqlTableShell;
use crate::mysql::types::AnyMysqlType;
use crate::mysql::vista::rhai_source::register_mysql_onto;
use crate::rhai_engine::RhaiExpr;

/// The MySQL vocabulary plus `self.with_condition(<expr>)` on the vista
/// handle, which routes the expression through `add_native_condition`. The
/// condition's key is its rendered SQL, so equal expressions narrow to equal
/// cache identities.
pub(crate) fn register_extensions(engine: &mut Engine) {
    register_mysql_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut Handle,
         cond: RhaiExpr<AnyMysqlType>|
         -> std::result::Result<Handle, Box<EvalAltResult>> {
            let key = cond.0.preview();
            h.with_native_condition("with_condition", NativeCondition::new(cond.0, key))
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
        let host = self.factory().script_host();
        let env = Env::new().var("me", record_to_dynamic(row));
        eval_ref_script(&host, script, env, row)
    }
}
