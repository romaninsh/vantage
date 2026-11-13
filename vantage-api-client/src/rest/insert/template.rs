//! The `api.insert.body` template: a Rhai expression compiled once and
//! evaluated per request.

#[cfg(feature = "rhai")]
mod enabled {
    use std::fmt;
    use std::ops::Range;
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
    /// send null. A name built at run time (`secret[x]`) is not seen, and a
    /// `secret.` inside a comment or a string literal is not a reference.
    fn secret_names(source: &str) -> Vec<String> {
        let is_ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut names = Vec::new();
        let skipped = non_code_ranges(source);
        for (at, _) in source.match_indices("secret") {
            if skipped.iter().any(|r| r.contains(&at)) {
                continue;
            }
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

    /// The byte ranges of Rhai `src` that are not code: `//` and (nested)
    /// `/* */` comments, and `"`, `'`, `` ` `` and raw `#"…"#` literals. A
    /// backtick string's `${ … }` interpolation is code.
    fn non_code_ranges(src: &str) -> Vec<Range<usize>> {
        let mut lexer = Lexer {
            bytes: src.as_bytes(),
            at: 0,
            ranges: Vec::new(),
        };
        lexer.code(false);
        lexer.ranges
    }

    /// The `#`s that open a raw string at the start of `rest`, if one does.
    fn raw_string_hashes(rest: &[u8]) -> Option<usize> {
        let hashes = rest.iter().take_while(|&&b| b == b'#').count();
        (hashes > 0 && rest.get(hashes) == Some(&b'"')).then_some(hashes)
    }

    /// Walks Rhai source by its lexical rules, noting the ranges that are
    /// not code.
    struct Lexer<'a> {
        bytes: &'a [u8],
        at: usize,
        ranges: Vec<Range<usize>>,
    }

    impl Lexer<'_> {
        fn rest(&self) -> &[u8] {
            &self.bytes[self.at.min(self.bytes.len())..]
        }

        /// Code to the end, or in an `interpolation` to just past the `}`
        /// that closes it.
        fn code(&mut self, interpolation: bool) {
            let mut depth = 0usize;
            while let Some(&byte) = self.bytes.get(self.at) {
                if self.non_code() {
                    continue;
                }
                self.at += 1;
                match byte {
                    b'{' => depth += 1,
                    b'}' if interpolation && depth == 0 => return,
                    b'}' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }

        /// Steps over the comment or literal at the cursor, noting its
        /// range. False when code is there.
        fn non_code(&mut self) -> bool {
            let start = self.at;
            let rest = self.rest();
            match rest.first() {
                Some(b'/') if rest.starts_with(b"//") => self.line_comment(),
                Some(b'/') if rest.starts_with(b"/*") => self.block_comment(),
                Some(b'"' | b'\'') => self.quoted(),
                Some(b'#') if raw_string_hashes(rest).is_some() => self.raw_string(),
                Some(b'`') => {
                    // Notes its literal pieces itself, around the code.
                    self.backtick();
                    return true;
                }
                _ => return false,
            }
            self.at = self.at.min(self.bytes.len());
            self.ranges.push(start..self.at);
            true
        }

        fn line_comment(&mut self) {
            let rest = self.rest();
            self.at += rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        }

        /// A `/* */` comment; they nest.
        fn block_comment(&mut self) {
            let mut depth = 0usize;
            while self.at < self.bytes.len() {
                let rest = self.rest();
                if rest.starts_with(b"/*") {
                    depth += 1;
                } else if rest.starts_with(b"*/") {
                    depth -= 1;
                } else {
                    self.at += 1;
                    continue;
                }
                self.at += 2;
                if depth == 0 {
                    return;
                }
            }
        }

        /// A `"` string or `'` character, with `\` escapes.
        fn quoted(&mut self) {
            let quote = self.bytes[self.at];
            self.at += 1;
            while let Some(&byte) = self.bytes.get(self.at) {
                self.at += if byte == b'\\' { 2 } else { 1 };
                if byte == quote {
                    return;
                }
            }
        }

        /// `#"…"#` with any number of `#`s: no escapes at all.
        fn raw_string(&mut self) {
            let hashes = raw_string_hashes(self.rest()).unwrap_or(0);
            self.at += hashes + 1;
            let mut closing = vec![b'"'];
            closing.resize(hashes + 1, b'#');
            let rest = self.rest();
            self.at += rest
                .windows(closing.len())
                .position(|w| w == closing)
                .map_or(rest.len(), |end| end + closing.len());
        }

        /// A backtick string: verbatim, but for the `\${` escape, and each
        /// `${ … }` in it is code.
        fn backtick(&mut self) {
            let mut literal_from = self.at;
            self.at += 1;
            while let Some(&byte) = self.bytes.get(self.at) {
                if byte == b'`' {
                    break;
                }
                let rest = self.rest();
                if rest.starts_with(b"\\${") {
                    self.at += 3;
                } else if rest.starts_with(b"${") {
                    self.at += 2;
                    self.ranges.push(literal_from..self.at);
                    self.code(true);
                    literal_from = self.at - 1;
                } else {
                    self.at += 1;
                }
            }
            self.at = (self.at + 1).min(self.bytes.len());
            self.ranges.push(literal_from..self.at);
        }
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
