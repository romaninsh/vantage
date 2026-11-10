//! The `api.insert.body` template: a Rhai expression compiled once and
//! evaluated per request.

#[cfg(feature = "rhai")]
mod enabled {
    use std::fmt;
    use std::sync::Arc;

    use indexmap::IndexMap;
    use rhai::serde::{from_dynamic, to_dynamic};
    use rhai::{AST, Dynamic, Engine, Map, Scope};
    use serde_json::Value;
    use vantage_core::{Result, error};
    use vantage_rhai::Limits;

    #[derive(Clone)]
    pub(crate) struct BodyTemplate {
        engine: Arc<Engine>,
        ast: Arc<AST>,
    }

    impl fmt::Debug for BodyTemplate {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("BodyTemplate")
        }
    }

    impl BodyTemplate {
        pub(crate) fn compile(source: &str) -> Result<Self> {
            let mut engine = Engine::new();
            Limits::background().apply(&mut engine);
            let ast = engine.compile_expression(source).map_err(|e| {
                error!(
                    "api.insert.body does not compile",
                    reason = e.to_string()
                )
            })?;
            Ok(Self {
                engine: Arc::new(engine),
                ast: Arc::new(ast),
            })
        }

        pub(crate) fn render(
            &self,
            records: &[Value],
            secrets: &IndexMap<String, String>,
        ) -> Result<Value> {
            let to_scope_err =
                |e: Box<rhai::EvalAltResult>| error!("api.insert.body failed", reason = e.to_string());
            let records_value = to_dynamic(records).map_err(to_scope_err)?;
            let record_value = to_dynamic(records.first()).map_err(to_scope_err)?;
            let secret_map: Map = secrets
                .iter()
                .map(|(k, v)| (k.as_str().into(), Dynamic::from(v.clone())))
                .collect();

            let mut scope = Scope::new();
            scope.push_constant("records", records_value);
            scope.push_constant("record", record_value);
            scope.push_constant("secret", secret_map);

            let out: Dynamic = self
                .engine
                .eval_ast_with_scope(&mut scope, &self.ast)
                .map_err(to_scope_err)?;
            from_dynamic(&out).map_err(to_scope_err)
        }
    }
}

#[cfg(not(feature = "rhai"))]
mod enabled {
    use indexmap::IndexMap;
    use serde_json::Value;
    use vantage_core::{Result, error};

    #[derive(Clone, Debug)]
    pub(crate) struct BodyTemplate;

    impl BodyTemplate {
        pub(crate) fn compile(_source: &str) -> Result<Self> {
            Err(error!(
                "api.insert.body needs the `rhai` feature of vantage-api-client"
            ))
        }

        pub(crate) fn render(
            &self,
            _records: &[Value],
            _secrets: &IndexMap<String, String>,
        ) -> Result<Value> {
            Err(error!(
                "api.insert.body needs the `rhai` feature of vantage-api-client"
            ))
        }
    }
}

pub(crate) use enabled::BodyTemplate;
