//! One error type for every slot. Every variant carries the slot source so a
//! validator can point at the YAML key that produced it.

use rhai::{EvalAltResult, ParseError, Position};

pub type Result<T> = std::result::Result<T, RhaiError>;

#[derive(Debug, thiserror::Error)]
pub enum RhaiError {
    #[error("{0}")]
    Syntax(Located),
    #[error("unknown name `{path}` in scope")]
    UnknownName { path: String, src: String },
    #[error("{0}")]
    Runtime(Located),
    #[error("expected {expected}, got {actual}")]
    WrongType {
        expected: &'static str,
        actual: String,
        src: String,
    },
    #[error("script exceeded its {limit} limit")]
    LimitExceeded { limit: &'static str, src: String },
}

impl RhaiError {
    pub fn src(&self) -> &str {
        match self {
            RhaiError::Syntax(l) | RhaiError::Runtime(l) => &l.src,
            RhaiError::UnknownName { src, .. }
            | RhaiError::WrongType { src, .. }
            | RhaiError::LimitExceeded { src, .. } => src,
        }
    }

    /// Classify a compile error. The message keeps rhai's own "Syntax error:"
    /// prefix, which `Engine::eval` used to add and which a separate compile
    /// step does not — so an author sees the same words either way.
    pub fn from_parse(src: &str, err: ParseError) -> Self {
        RhaiError::Syntax(Located::new(
            src,
            err.position(),
            format!("Syntax error: {}", err.err_type()),
        ))
    }

    // Boxed by rhai's own choice: every `eval`/`run` returns
    // `Result<_, Box<EvalAltResult>>`, so taking it boxed is what callers have.
    #[allow(clippy::boxed_local)]
    pub fn from_eval(src: &str, err: Box<EvalAltResult>) -> Self {
        Self::from_eval_at(src, err, None)
    }

    /// Classify an eval error. `remap` shifts the reported position into the
    /// coordinates of `src` — a template hole's AST reports positions relative
    /// to the hole, but `src` is the whole template.
    #[allow(clippy::boxed_local)]
    pub fn from_eval_at(src: &str, err: Box<EvalAltResult>, remap: Option<(usize, usize)>) -> Self {
        // A failure inside a script `fn` arrives wrapped, and the wrapper is
        // what `match` would see — unwrap so the real cause is classified.
        let err = match *err {
            EvalAltResult::ErrorInFunctionCall(_, _, inner, _)
            | EvalAltResult::ErrorInModule(_, inner, _) => inner,
            other => Box::new(other),
        };
        let src_s = src.to_string();
        match *err {
            EvalAltResult::ErrorTooManyOperations(_) => RhaiError::LimitExceeded {
                limit: "operations",
                src: src_s,
            },
            EvalAltResult::ErrorDataTooLarge(..) => RhaiError::LimitExceeded {
                limit: "size",
                src: src_s,
            },
            EvalAltResult::ErrorStackOverflow(_) => RhaiError::LimitExceeded {
                limit: "call depth",
                src: src_s,
            },
            EvalAltResult::ErrorVariableNotFound(name, _) => RhaiError::UnknownName {
                path: name,
                src: src_s,
            },
            EvalAltResult::ErrorParsing(err_type, pos) => RhaiError::Syntax(Located::at(
                src,
                pos,
                format!("Syntax error: {err_type}"),
                remap,
            )),
            other => {
                let pos = other.position();
                // `to_string()` already embeds the position; keep the message
                // alone so `Located` is not printed twice.
                let message = other.to_string();
                let message = match message.find(" (line ") {
                    Some(cut) => message[..cut].to_string(),
                    None => message,
                };
                RhaiError::Runtime(Located::at(src, pos, message, remap))
            }
        }
    }

    /// Re-anchor a fragment's error onto the text that contains it. A template
    /// hole compiles in isolation, so its position and source excerpt refer to
    /// the hole; `at` is where the hole begins in `src`.
    pub fn rebase(self, src: &str, at: (usize, usize)) -> Self {
        let shift = |l: Located| {
            let (line, column) = if l.line == 0 {
                (0, 0)
            } else if l.line == 1 {
                (at.0, at.1 + l.column.saturating_sub(1))
            } else {
                (at.0 + l.line - 1, l.column)
            };
            Located {
                src: src.to_string(),
                line,
                column,
                message: l.message,
            }
        };
        match self {
            RhaiError::Syntax(l) => RhaiError::Syntax(shift(l)),
            RhaiError::Runtime(l) => RhaiError::Runtime(shift(l)),
            RhaiError::UnknownName { path, .. } => RhaiError::UnknownName {
                path,
                src: src.to_string(),
            },
            RhaiError::WrongType {
                expected, actual, ..
            } => RhaiError::WrongType {
                expected,
                actual,
                src: src.to_string(),
            },
            RhaiError::LimitExceeded { limit, .. } => RhaiError::LimitExceeded {
                limit,
                src: src.to_string(),
            },
        }
    }

    pub fn wrong_type(src: &str, expected: &'static str, value: &rhai::Dynamic) -> Self {
        RhaiError::WrongType {
            expected,
            actual: value.type_name().to_string(),
            src: src.to_string(),
        }
    }
}

/// A message anchored to a line/column in the slot source. `Display` prints the
/// message, the offending source line, and a caret under the column.
#[derive(Debug)]
pub struct Located {
    pub src: String,
    /// 1-based; 0 when rhai reported no position.
    pub line: usize,
    /// 1-based; 0 when rhai reported no position.
    pub column: usize,
    pub message: String,
}

impl Located {
    pub fn new(src: &str, pos: Position, message: String) -> Self {
        Self::at(src, pos, message, None)
    }

    /// `remap` is the (line, column) in `src` at which the reporting fragment
    /// begins, both 1-based. A position on the fragment's first line is offset
    /// by the column too; later lines only shift by line.
    pub fn at(src: &str, pos: Position, message: String, remap: Option<(usize, usize)>) -> Self {
        let mut line = pos.line().unwrap_or(0);
        let mut column = pos.position().unwrap_or(0);
        if let Some((base_line, base_col)) = remap
            && line > 0
        {
            if line == 1 {
                column = base_col + column.saturating_sub(1);
            }
            line = base_line + line - 1;
        }
        Located {
            src: src.to_string(),
            line,
            column,
            message,
        }
    }
}

impl std::fmt::Display for Located {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if self.line == 0 {
            return Ok(());
        }
        let Some(line) = self.src.lines().nth(self.line - 1) else {
            return Ok(());
        };
        write!(
            f,
            " (line {}, column {})\n  | {}\n  | ",
            self.line, self.column, line
        )?;
        for _ in 1..self.column {
            f.write_str(" ")?;
        }
        f.write_str("^")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_error_shows_line_and_caret() {
        let engine = rhai::Engine::new();
        let src = "let a = 1;\nlet b = a +;";
        let err = engine.compile(src).unwrap_err();
        let e = RhaiError::from_parse(src, err);
        let text = e.to_string();
        assert!(matches!(e, RhaiError::Syntax(_)));
        assert!(text.contains("line 2"), "{text}");
        assert!(text.contains("| let b = a +;"), "{text}");
        assert!(
            text.lines().last().unwrap().trim_end().ends_with('^'),
            "{text}"
        );
    }

    #[test]
    fn operations_limit_maps_to_limit_exceeded() {
        let mut engine = rhai::Engine::new();
        engine.set_max_operations(1_000);
        let src = "while true {}";
        let err = engine.run(src).unwrap_err();
        let e = RhaiError::from_eval(src, err);
        assert!(matches!(
            e,
            RhaiError::LimitExceeded {
                limit: "operations",
                ..
            }
        ));
        assert_eq!(e.src(), src);
    }

    #[test]
    fn variable_not_found_maps_to_unknown_name() {
        let engine = rhai::Engine::new();
        let src = "nope + 1";
        let err = engine.eval::<rhai::Dynamic>(src).unwrap_err();
        let e = RhaiError::from_eval(src, err);
        match e {
            RhaiError::UnknownName { path, .. } => assert_eq!(path, "nope"),
            other => panic!("expected UnknownName, got {other:?}"),
        }
    }

    #[test]
    fn runtime_error_carries_source() {
        let engine = rhai::Engine::new();
        let src = "throw \"boom\"";
        let err = engine.run(src).unwrap_err();
        let e = RhaiError::from_eval(src, err);
        assert!(matches!(e, RhaiError::Runtime(_)));
        assert_eq!(e.src(), src);
        assert!(e.to_string().contains("boom"));
    }
}
