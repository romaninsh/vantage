//! Rhai entry points for MongoDB vistas: the `rhai:` pipeline script, the
//! `modify:` script, and the host shared by `modify:` and scripted
//! references.

use std::sync::LazyLock;

use bson::{Bson, Document};
use vantage_core::{Context, Result, error};
use vantage_rhai::rhai::{Array, Dynamic, Map};
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_vista::Vista;
use vantage_vista::rhai::{DataVocab, Terminals, eval_modify_script, script_tables};

use crate::rhai_engine::{MongoVocab, dynamic_to_bson};
use crate::vista::factory::MongoVistaFactory;
use crate::vista::source::register_extensions;
use crate::vista::spec::MongoVistaSpec;
use crate::vista::spec::pipeline::{check_stages, not_a_stage};

/// Constructors only: a pipeline script builds data, it reaches no tables.
fn pipeline_host() -> &'static Host {
    static HOST: LazyLock<Host> = LazyLock::new(|| {
        Host::builder(Limits::background())
            .vocab(MongoVocab)
            .build()
    });
    &HOST
}

/// Run a `rhai:` script and return the pipeline it evaluates to. `args` are
/// in scope as the `args` map. The stages are validated like YAML
/// `pipeline:` stages.
pub(crate) fn eval_pipeline(code: &str, args: &[(String, String)]) -> Result<Vec<Document>> {
    let mut map = Map::new();
    for (k, v) in args {
        map.insert(k.as_str().into(), v.clone().into());
    }
    let env = Env::new().var("args", Dynamic::from_map(map));
    let value = pipeline_host()
        .compile(&Block::from(code))
        .and_then(|script| script.eval(&env))
        .context(error!("rhai pipeline script failed"))?;
    let type_name = value.type_name();
    let items = value.try_cast::<Array>().ok_or_else(|| {
        error!(
            "rhai pipeline script must evaluate to an array of stages",
            got = type_name
        )
    })?;
    let stages = items
        .into_iter()
        .enumerate()
        .map(|(index, stage)| {
            match dynamic_to_bson(stage)
                .with_context(|| error!("pipeline stage is not BSON", stage = index))?
            {
                Bson::Document(doc) => Ok(doc),
                _ => Err(not_a_stage(index)),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    check_stages(&stages)?;
    Ok(stages)
}

/// Run a `modify:` script against a built vista, exposed as `self`.
pub(crate) fn apply_modify(factory: &MongoVistaFactory, vista: Vista, code: &str) -> Result<Vista> {
    let host = script_host(factory.clone());
    eval_modify_script(&host, code, vista)
}

/// The shell's MongoDB vocabulary first, then vantage-vista's describe-only
/// data verbs, with `table(name)` building the named vista through
/// `factory`. With a spec resolver attached a name it doesn't know is
/// not-found; without one, every name is a bare collection.
pub(crate) fn script_host(factory: MongoVistaFactory) -> Host {
    let specs = factory.resolver.clone();
    let tables = script_tables(factory, specs, Some(|name| MongoVistaSpec::new(name)));
    let data = DataVocab::new(Terminals::Describe).with_resolver(tables);
    Host::builder(Limits::background())
        .vocab_fn(register_extensions)
        .vocab(data)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bson::doc;

    #[test]
    fn pipeline_script_sees_args() {
        let code = r#"
            let stages = [];
            if args.cat != "" { stages.push(#{"$match": #{category: args.cat}}); }
            stages.push(#{"$sort": mongo::doc([["n", -1], ["_id", 1]])});
            stages
        "#;
        let stages = eval_pipeline(code, &[("cat".into(), "cake".into())]).unwrap();
        assert_eq!(
            stages,
            vec![
                doc! { "$match": { "category": "cake" } },
                doc! { "$sort": { "n": -1_i64, "_id": 1_i64 } },
            ]
        );
        let stages = eval_pipeline(code, &[("cat".into(), String::new())]).unwrap();
        assert_eq!(stages.len(), 1);
    }

    #[test]
    fn pipeline_script_must_return_stages() {
        let err = eval_pipeline(r#"#{"$match": #{}}"#, &[]).unwrap_err();
        assert!(err.to_string().contains("array of stages"), "{err}");
        let err = eval_pipeline("[]", &[]).unwrap_err();
        assert!(err.to_string().contains("at least one stage"), "{err}");
        let err = eval_pipeline(r#"[#{"$match": #{}}, 1]"#, &[]).unwrap_err();
        assert!(err.to_string().contains("stage: 1"), "{err}");
    }

    #[test]
    fn pipeline_script_is_bounded() {
        let err = eval_pipeline("loop {}", &[]).unwrap_err();
        assert!(format!("{err:?}").contains("limit"), "{err:?}");
    }
}
