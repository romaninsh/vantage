//! Rhai on the shell: the `with_condition` verb and scripted reference
//! traversal.
//!
//! MongoDB has no current-record anchor to offer (SurrealDB's `me` is a graph
//! anchor), so a reference build-script instead sees the parent row twice:
//! as `row`, like every backend, and as `me`. Row values arrive as the vista
//! surfaces them, so an `ObjectId` is its hex text: `where("client_id",
//! me._id)` matches either spelling, while a raw filter needs
//! `mongo::oid(me._id)`.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_rhai::Env;
use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult};
use vantage_types::Record;
use vantage_vista::rhai::{Handle, RuleHandle, eval_ref_script, record_to_dynamic};
use vantage_vista::{NativeCondition, Vista};

use super::{MongoTableShell, conditions};
use crate::condition::MongoCondition;
use crate::rhai_engine::{dynamic_to_document, register_mongo_onto};
use crate::vista::scripting::script_host;

/// The MongoDB vocabulary plus `self.with_condition(<map or mongo::doc()>)` on the
/// vista handle, routed through `add_native_condition`. The filter's field
/// names are raw BSON paths; its key is the filter's canonical JSON, so equal
/// filters narrow to equal cache identities.
pub(crate) fn register_extensions(engine: &mut Engine) {
    register_mongo_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut Handle, filter: Dynamic| -> std::result::Result<Handle, Box<EvalAltResult>> {
            let filter = dynamic_to_document(filter).map_err(|e| e.to_string())?;
            let key = conditions::native_key(&filter);
            h.with_native_condition("with_condition", NativeCondition::new(filter, key))
                .map_err(|e| e.to_string().into())
        },
    );
}

/// The MongoDB vocabulary for rule scripts: the `mongo::` constructors, and
/// `table.with_condition(<map or mongo::doc()>)` on the [`RuleHandle`],
/// which narrows the table by the filter document. Register it after
/// [`register_rule_vocab`](vantage_vista::rhai::register_rule_vocab).
pub fn register_rule_conditions(engine: &mut Engine) {
    register_mongo_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut RuleHandle,
         filter: Dynamic|
         -> std::result::Result<RuleHandle, Box<EvalAltResult>> {
            let filter = dynamic_to_document(filter).map_err(|e| e.to_string())?;
            let key = conditions::native_key(&filter);
            h.native_condition(Box::new(MongoCondition::Doc(filter)), key)
                .map_err(|e| e.to_string())?;
            Ok(h.clone())
        },
    );
}

impl MongoTableShell {
    /// Build a reference's traversal target by evaluating its `build_script`
    /// with the parent row in scope as `row` and `me`; `table(name)` builds
    /// the named vista through this shell's factory.
    pub(super) fn get_ref_via_script(
        &self,
        script: &str,
        row: &Record<CborValue>,
    ) -> Result<Vista> {
        let host = script_host(self.factory());
        let env = Env::new().var("me", record_to_dynamic(row));
        eval_ref_script(&host, script, env, row)
    }
}
