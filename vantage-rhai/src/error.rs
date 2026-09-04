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

    pub fn from_parse(src: &str, err: ParseError) -> Self {
        RhaiError::Syntax(Located::new(
            src,
            err.position(),
            err.err_type().to_string(),
        ))
    }

    // Boxed by rhai's own choice: every `eval`/`run` returns
    // `Result<_, Box<EvalAltResult>>`, so taking it boxed is what callers have.
    #[allow(clippy::boxed_local)]
    pub fn from_eval(src: &str, err: Box<EvalAltResult>) -> Self {
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
            EvalAltResult::ErrorParsing(err_type, pos) => {
                RhaiError::Syntax(Located::new(src, pos, err_type.to_string()))
            }
            other => {
                let pos = other.position();
                RhaiError::Runtime(Located::new(src, pos, other.to_string()))
            }
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
        Located {
            src: src.to_string(),
            line: pos.line().unwrap_or(0),
            column: pos.position().unwrap_or(0),
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
