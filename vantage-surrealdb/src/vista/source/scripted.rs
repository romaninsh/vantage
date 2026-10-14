//! Rhai on the shell: the SurrealDB vocabulary, the `with_condition` verb,
//! the `surreal::me` anchor, and scripted reference traversal.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_rhai::Env;
use vantage_rhai::rhai::{Engine, EvalAltResult};
use vantage_types::{Entity, Record};
use vantage_vista::rhai::{Handle, eval_ref_script};
use vantage_vista::{NativeCondition, Vista};

use super::SurrealTableShell;
use crate::rhai_engine::{RhaiExpr, register_surreal_onto};
use crate::types::AnySurrealType;

/// The SurrealDB vocabulary for scripts over a SurrealDB vista:
/// - the query-building constructors and the `me` anchor in the `surreal::`
///   namespace, and the operators (see [`register_surreal_onto`]);
/// - `self.with_condition(<expr>)` on the vista handle, routed through
///   `add_native_condition`. The condition's key is its rendered SurrealQL,
///   so equal expressions narrow to equal cache identities.
pub(crate) fn register_extensions(engine: &mut Engine) {
    register_surreal_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut Handle, cond: RhaiExpr| -> std::result::Result<Handle, Box<EvalAltResult>> {
            let key = cond.0.preview();
            h.with_native_condition("with_condition", NativeCondition::new(cond.0, key))
                .map_err(|e| e.to_string().into())
        },
    );
}

impl<E> SurrealTableShell<E>
where
    E: Entity<AnySurrealType> + 'static,
{
    /// Build a reference's traversal target by evaluating its `build_script`
    /// with the parent `row` in scope; `table(name)` builds the named vista
    /// through this shell's factory.
    pub(super) fn get_ref_via_script(
        &self,
        script: &str,
        row: &Record<CborValue>,
    ) -> Result<Vista> {
        let host = self.factory().script_host();
        eval_ref_script(&host, script, Env::new(), row)
    }
}
