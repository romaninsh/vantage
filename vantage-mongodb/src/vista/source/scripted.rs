//! Rhai on the shell: the `with_condition` verb and scripted reference
//! traversal.
//!
//! MongoDB has no current-record anchor to offer (SurrealDB's `me` is a graph
//! anchor), so a reference build-script instead sees the parent row twice:
//! as `row`, like every backend, and as `me`. Row values arrive as the vista
//! surfaces them, so an `ObjectId` is its hex text: `where("client_id",
//! me._id)` matches either spelling, while a raw filter needs
//! `oid(me._id)`.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_rhai::Env;
use vantage_rhai::rhai::{Dynamic, Engine, EvalAltResult};
use vantage_types::Record;
use vantage_vista::{Handle, TableShell, Vista};

use super::MongoTableShell;
use crate::rhai_engine::{dynamic_to_document, register_mongo_onto};
use crate::vista::scripting::script_host;

/// The MongoDB vocabulary plus `self.with_condition(<map or doc()>)` on the
/// vista handle. The filter's field names are raw BSON paths.
pub(super) fn register_extensions(engine: &mut Engine) {
    register_mongo_onto(engine);
    engine.register_fn(
        "with_condition",
        |h: &mut Handle, filter: Dynamic| -> std::result::Result<Handle, Box<EvalAltResult>> {
            let filter = dynamic_to_document(filter).map_err(|e| e.to_string())?;
            h.with_base_vista("with_condition", |vista| vista.add_raw_condition(filter))
                .map_err(|e| e.to_string().into())
        },
    );
}

impl MongoTableShell {
    /// Build a reference's traversal target by evaluating its `build_script`;
    /// `table(name)` builds the named vista through this shell's factory.
    pub(super) fn get_ref_via_script(
        &self,
        script: &str,
        row: &Record<CborValue>,
    ) -> Result<Vista> {
        let host = script_host(self, self.factory());
        let env = self
            .rhai_env(Env::new())
            .var("me", vantage_vista::record_to_dynamic(row));
        vantage_vista::eval_ref_script(&host, script, env, row)
    }
}
