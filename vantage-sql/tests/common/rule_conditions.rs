//! A rule target that only takes `with_condition` and renders it, so a
//! rule script's condition can be checked without a database.

use std::any::Any;
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_rhai::rhai::Engine;
use vantage_rhai::{Env, Host, Limits};
use vantage_vista::FilterOp;
use vantage_vista::rhai::{
    HookOn, RuleHandle, RuleHookFn, RuleTarget, eval_rule_script, register_rule_vocab,
};

type Render = fn(Box<dyn Any + Send>) -> String;

#[derive(Clone)]
struct Rendered {
    render: Render,
    out: Arc<Mutex<Vec<String>>>,
}

impl RuleTarget for Rendered {
    fn where_value(&mut self, _: &str, _: FilterOp, _: CborValue) -> vantage_core::Result<String> {
        unreachable!("the script only calls with_condition")
    }

    fn native_condition(
        &mut self,
        condition: Box<dyn Any + Send>,
        _preview: String,
    ) -> vantage_core::Result<String> {
        let sql = (self.render)(condition);
        self.out.lock().unwrap().push(sql.clone());
        Ok(sql)
    }

    fn hook(&mut self, _: HookOn, _: RuleHookFn) -> vantage_core::Result<()> {
        unreachable!("the script only calls with_condition")
    }

    fn remove_column(&mut self, _: &str) -> vantage_core::Result<()> {
        unreachable!("the script only calls with_condition")
    }
}

/// Run `script` on a host with `register` after the rule vocabulary; each
/// condition the script hands the table, rendered by `render`.
pub fn rule_conditions(register: fn(&mut Engine), render: Render, script: &str) -> Vec<String> {
    let target = Rendered {
        render,
        out: Arc::default(),
    };
    let handle = RuleHandle::new(Box::new(target.clone()));
    let host = Host::builder(Limits::background())
        .vocab_fn(register_rule_vocab)
        .vocab_fn(register)
        .build();
    eval_rule_script(&host, script, Env::new(), &handle).unwrap();
    target.out.lock().unwrap().clone()
}
