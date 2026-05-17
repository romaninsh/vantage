//! Token, operator, and selector types produced by the parser.
//!
//! The parser turns argv positionals into a `Vec<Token>`; the runner
//! consumes them. Keeping the data types in one place makes the grammar
//! easy to read at a glance.

use ciborium::Value as CborValue;

/// Whether the current state is a list of records or a single record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    Single,
}

/// Comparison + membership operators in the universal Vista vocabulary
/// (stage 5 of the Vista roadmap). The parser recognises every variant;
/// only `Eq` currently translates to a real Vista call — see
/// `vista_cli::run` for the stub paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Like,
    In,
    IsNull,
    IsNotNull,
}

impl Op {
    pub fn name(self) -> &'static str {
        match self {
            Op::Eq => "eq",
            Op::Ne => "ne",
            Op::Lt => "lt",
            Op::Lte => "lte",
            Op::Gt => "gt",
            Op::Gte => "gte",
            Op::Like => "like",
            Op::In => "in",
            Op::IsNull => "null",
            Op::IsNotNull => "notnull",
        }
    }

    /// Parse an op suffix (the part after `:` in `field:op`).
    pub fn parse(s: &str) -> Option<Op> {
        match s {
            "eq" => Some(Op::Eq),
            "ne" => Some(Op::Ne),
            "lt" => Some(Op::Lt),
            "lte" => Some(Op::Lte),
            "gt" => Some(Op::Gt),
            "gte" => Some(Op::Gte),
            "like" => Some(Op::Like),
            "in" => Some(Op::In),
            "null" => Some(Op::IsNull),
            "notnull" => Some(Op::IsNotNull),
            _ => None,
        }
    }

    pub fn is_nullary(self) -> bool {
        matches!(self, Op::IsNull | Op::IsNotNull)
    }
}

/// Sort direction inside a `[+field]` / `[-field]` bracket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Asc,
    Desc,
}

/// Aggregate verb in an `@op[:field]` token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateOp {
    Sum,
    Max,
    Min,
    Count,
}

impl AggregateOp {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sum" => Some(AggregateOp::Sum),
            "max" => Some(AggregateOp::Max),
            "min" => Some(AggregateOp::Min),
            "count" => Some(AggregateOp::Count),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AggregateOp::Sum => "sum",
            AggregateOp::Max => "max",
            AggregateOp::Min => "min",
            AggregateOp::Count => "count",
        }
    }
}

/// The `[…]` selector — sort + slice combined. Either component may be
/// absent. If both are present, sort applies *before* slicing — so
/// `[+salary:0]` picks the highest-salary row, not row 0 of the original
/// order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    pub sort: Option<(String, Direction)>,
    pub slice: Option<Slice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slice {
    /// `[N]` or `[+sort:N]` — pick row N and narrow to single mode.
    Index(usize),
    /// `[start:end]` or `[:end]` or `[start:]` or `[:]` — half-open
    /// range, stays in list mode. `end = None` means open-ended.
    Range { start: usize, end: Option<usize> },
}

impl Selector {
    pub fn is_empty(&self) -> bool {
        self.sort.is_none() && self.slice.is_none()
    }
}

/// One parsed argv token. Glued forms (`users[0]`, `:rel[0]`,
/// `field=v[0]`, `=col1[0]`) attach the selector as the second tuple
/// field on each variant that supports a trailing bracket.
///
/// Only `PartialEq` is derived (not `Eq`) because `ciborium::Value`
/// includes `Float`, which is itself only `PartialEq`.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// `users` / `user` — model name plus optional selector.
    ModelName(String, Option<Selector>),
    /// `arn:…`, `user:abc123`, `urn:…` — anything backend-specific the
    /// `ModelFactory::for_locator` recognises. Stored verbatim.
    Locator(String),
    /// `field=value`, `field:op=value`, `field:null`, `field:notnull`.
    /// `value` is `None` for nullary ops, otherwise the parsed CBOR.
    OpCondition {
        field: String,
        op: Op,
        value: Option<CborValue>,
        selector: Option<Selector>,
    },
    /// `:relation` — traverse a typed relation.
    Relation(String, Option<Selector>),
    /// Standalone `[…]` — sort and/or slice on the current vista.
    Bracket(Selector),
    /// `=col1,col2,…` — column override for the next render.
    Columns(Vec<String>, Option<Selector>),
    /// `?keyword` or `?"two words"` — full-text-ish search across
    /// `SEARCHABLE`-flagged columns.
    Search(String),
    /// `@sum:price`, `@count`, etc. Aggregates short-circuit normal
    /// list/single rendering — the runner produces a single scalar.
    Aggregate {
        op: AggregateOp,
        field: Option<String>,
    },
}
