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

    use crate::redact::Redaction;

    #[derive(Clone)]
    pub(crate) struct BodyTemplate {
        engine: Arc<Engine>,
        ast: Arc<AST>,
        /// Every `secret.<name>` and `secret["<name>"]` the source reads.
        secret_names: Vec<String>,
    }

    /// The secret names `source` reads, found by scanning its text: a miss
    /// in a Rhai map is a silent `()`, so an unbound secret would otherwise
    /// send null. A name built at run time (`secret[x]`) is not seen.
    fn secret_names(source: &str) -> Vec<String> {
        let is_ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut names = Vec::new();
        for (at, _) in source.match_indices("secret") {
            let before = source[..at].chars().next_back();
            if before.is_some_and(|c| is_ident(c) || c == '.') {
                continue;
            }
            let rest = source[at + "secret".len()..].trim_start();
            let name = if let Some(rest) = rest.strip_prefix('.') {
                let rest = rest.trim_start();
                let end = rest.find(|c| !is_ident(c)).unwrap_or(rest.len());
                &rest[..end]
            } else if let Some(rest) = rest.strip_prefix('[') {
                let rest = rest.trim_start();
                match rest.chars().next() {
                    Some(quote @ ('"' | '\'')) => {
                        let rest = &rest[1..];
                        &rest[..rest.find(quote).unwrap_or(0)]
                    }
                    _ => continue,
                }
            } else {
                continue;
            };
            if !name.is_empty() && !names.iter().any(|n| n == name) {
                names.push(name.to_string());
            }
        }
        names
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
            // A template has no business writing to the process's stdout.
            engine.on_print(|_| {});
            engine.on_debug(|_, _, _| {});
            let ast = engine
                .compile_expression(source)
                .map_err(|e| error!("api.insert.body does not compile", reason = e.to_string()))?;
            Ok(Self {
                engine: Arc::new(engine),
                ast: Arc::new(ast),
                secret_names: secret_names(source),
            })
        }

        pub(crate) fn render(
            &self,
            records: &[Value],
            secrets: &IndexMap<String, String>,
            redaction: &Redaction,
        ) -> Result<Value> {
            if let Some(name) = self.secret_names.iter().find(|n| !secrets.contains_key(*n)) {
                return Err(error!(
                    "api.insert.body reads a secret that is not bound; bind it with \
                     with_insert_secrets (or with_secrets)",
                    secret = name.as_str()
                ));
            }
            let to_scope_err = |e: Box<rhai::EvalAltResult>| {
                error!(
                    "api.insert.body failed",
                    reason = redaction.text(&e.to_string())
                )
            };
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

    use crate::redact::Redaction;

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
            _redaction: &Redaction,
        ) -> Result<Value> {
            Err(error!(
                "api.insert.body needs the `rhai` feature of vantage-api-client"
            ))
        }
    }
}

pub(crate) use enabled::BodyTemplate;
