//! Token, operator, and selector types produced by the parser.
//!
//! The parser turns argv positionals into `Token`s; the runner consumes them.

use ciborium::Value as CborValue;

/// Whether the current state is a list of records or a single record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    Single,
}

/// Comparison and membership operators. The parser recognises every
/// variant; only `Eq` currently reaches Vista, the rest are reported
/// through `Renderer::note_stub`.
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
/// `field=v[0]`, `=col1[0]`) carry their trailing bracket as a selector.
///
/// No `Eq`: `ciborium::Value` holds floats.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// `users` / `user` — model name plus optional selector.
    ModelName(String, Option<Selector>),
    /// `arn:…`, `user:abc123`, `urn:…`, stored verbatim for
    /// `ModelFactory::for_locator`.
    Locator(String),
    /// `field=value`, `field:op=value`, `field:null`, `field:notnull`.
    /// `value` is `None` for nullary ops. `value_raw` holds the user's
    /// text when the value was auto-detected, so the runner can re-coerce
    /// it against the column's declared type; it is `None` for `#literal`
    /// values, `:in` lists and nullary ops.
    OpCondition {
        field: String,
        op: Op,
        value: Option<CborValue>,
        value_raw: Option<String>,
        selector: Option<Selector>,
    },
    /// `:relation` — traverse a typed relation.
    Relation(String, Option<Selector>),
    /// Standalone `[…]` — sort and/or slice on the current vista.
    Bracket(Selector),
    /// `=col1,col2,…` — column override for the next render.
    Columns(Vec<String>, Option<Selector>),
    /// `?keyword` or `?"two words"` — search across searchable columns.
    Search(String),
    /// `@sum:price`, `@count`, etc. The runner renders a single scalar.
    Aggregate {
        op: AggregateOp,
        field: Option<String>,
    },
}
