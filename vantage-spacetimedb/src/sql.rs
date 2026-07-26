//! Statement construction for the SpacetimeDB SQL dialect.
//!
//! Every statement this driver sends is built here, and every value reaches a
//! query through [`Literal`]. That single-entrance rule is the whole point of
//! the module.
//!
//! # Why the values are rendered rather than bound
//!
//! The sibling SQL drivers compose `vantage_expressions::Expression`, which
//! keeps a template and a parameter list apart until the driver binds them —
//! `$_arg1` for SurrealDB, `?` for SQLite. A value never enters the query text,
//! so quoting cannot go wrong.
//!
//! SpacetimeDB offers nowhere to bind. Its HTTP API documents the request body
//! as *"SQL queries, separated by `;`"*: one string, no parameter array, no
//! prepared statements, and subscriptions likewise take a query string. Values
//! therefore have to be rendered into the text.
//!
//! What follows from that is not "so concatenate carefully at each call site" —
//! it is the opposite. Because rendering is unavoidable, it must happen in
//! exactly one place, and that place must refuse whatever it cannot prove safe.
//! A `Literal` cannot be constructed from an unvalidated string at all, so a
//! caller has no way to smuggle one in.
//!
//! # What "cannot prove safe" rules out
//!
//! The escape hatch that mattered was hexadecimal. Identity columns take
//! `0x…` literals, which are unquoted by definition, so an earlier version
//! emitted any id beginning `0x` verbatim — and an id is caller-controlled.
//! `0x0 OR 1=1; DELETE FROM account` rendered into a `DELETE … WHERE id = …`
//! produced two statements, the second of which the endpoint duly ran.
//! [`Literal::hex`] now checks every remaining character is a hex digit.
//!
//! The same reasoning governs bare numerics: a literal is emitted unquoted only
//! once it has parsed as a number, never because it merely looked like one.

use ciborium::Value as CborValue;
use spacetimedb_sats::AlgebraicType;
use vantage_core::{Result, error};
use vantage_vista::FilterOp;

/// A validated identifier — a table, view or column name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ident(String);

impl Ident {
    /// Validate a name from the module schema.
    ///
    /// Module identifiers are constrained to `[A-Za-z0-9_]` at the source, so
    /// the plain case is the common one. Anything else is double-quoted, and a
    /// name carrying a quote or a control character is refused rather than
    /// escaped: no legitimate schema produces one, so it means the schema is not
    /// the schema we think it is.
    pub(crate) fn new(name: &str) -> Result<Self> {
        if name.is_empty() {
            return Err(error!("an empty identifier cannot be quoted"));
        }
        if name.chars().any(|c| c.is_control() || c == '"') {
            return Err(error!(
                "identifier contains characters no SpacetimeDB schema declares",
                name = name.to_string()
            ));
        }
        Ok(Self(
            if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                name.to_string()
            } else {
                format!("\"{name}\"")
            },
        ))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// A value already proven safe to place in a statement.
///
/// There is no constructor taking pre-rendered SQL, which is what stops a caller
/// bypassing the checks below.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Literal(String);

impl Literal {
    /// A quoted string, with embedded quotes doubled — the only escape the
    /// dialect defines.
    ///
    /// A NUL is refused outright. Doubling quotes handles the quote breakout,
    /// but a NUL can truncate a string at any C boundary the request crosses,
    /// and a truncated statement is a different statement.
    pub(crate) fn text(value: &str) -> Result<Self> {
        if value.contains('\0') {
            return Err(error!("a string literal cannot contain a NUL"));
        }
        Ok(Self(format!("'{}'", value.replace('\'', "''"))))
    }

    pub(crate) fn integer(value: i128) -> Self {
        Self(value.to_string())
    }

    pub(crate) fn float(value: f64) -> Self {
        Self(value.to_string())
    }

    pub(crate) fn boolean(value: bool) -> Self {
        Self(value.to_string())
    }

    /// A `0x…` literal, as identity and connection-id columns take.
    ///
    /// Unquoted, and therefore the one shape that must be checked character by
    /// character: everything after the prefix has to be a hex digit. Without
    /// that, an id is a hole straight into the statement.
    pub(crate) fn hex(value: &str) -> Result<Self> {
        let digits = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
            .ok_or_else(|| error!("not a hex literal", value = value.to_string()))?;
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(error!(
                "a hex literal must be `0x` followed by hex digits only",
                value = value.to_string()
            ));
        }
        Ok(Self(value.to_string()))
    }

    /// Render a decoded value for a `WHERE` comparison or an assignment.
    pub(crate) fn from_cbor(value: &CborValue) -> Result<Self> {
        Ok(match value {
            // A tag is a CBOR annotation, not part of the value: Vista's rhai
            // bridge tags datetimes and record references on their way through.
            // Every other driver drops it and keeps the payload; rejecting it
            // here turned a scripted datetime condition into "value cannot be
            // expressed as a SpacetimeDB SQL literal".
            CborValue::Tag(_, inner) => Self::from_cbor(inner)?,
            CborValue::Bool(b) => Self::boolean(*b),
            CborValue::Integer(i) => Self::integer(i128::from(*i)),
            CborValue::Float(f) => Self::float(*f),
            CborValue::Text(s) => Self::text(s)?,
            CborValue::Null => {
                return Err(
                    error!("SpacetimeDB SQL has no null literal to compare against")
                        .mark_unimplemented(),
                );
            }
            other => {
                return Err(error!(
                    "value cannot be expressed as a SpacetimeDB SQL literal",
                    value = format!("{other:?}")
                )
                .mark_unimplemented());
            }
        })
    }

    /// Render a Vista id for the column that holds it.
    ///
    /// Vista fixes ids as strings; the column addressed by one often is not. The
    /// decision to quote therefore comes from the column's declared type, not
    /// from the shape of the string — a text key of `"42"` is still text, and
    /// emitting it bare matches nothing while reading like success.
    ///
    /// When the schema does not describe the column, the id is quoted. That is
    /// the safe direction: a quoted literal can fail to match, whereas an
    /// unquoted one can change the statement.
    pub(crate) fn for_id(column_type: Option<&AlgebraicType>, id: &str) -> Result<Self> {
        match column_type {
            Some(ty) if ty.is_integer() => id.parse::<i128>().map(Self::integer).map_err(|_| {
                error!(
                    "id is not a whole number, but its column is",
                    id = id.to_string()
                )
            }),
            Some(ty) if ty.is_float() => id
                .parse::<f64>()
                .map(Self::float)
                .map_err(|_| error!("id is not a number, but its column is", id = id.to_string())),
            // The special products, each rendered the way `value.rs` renders it
            // on the way *out*. That correspondence is the whole requirement: an
            // id is a string this driver itself produced from a row, so a write
            // path that disagrees with the read path cannot address the row it
            // was just handed.
            //
            // Treating every special type as hex — which `is_special()` invites,
            // since it covers all five — broke exactly that. A `Timestamp` key
            // lists as microseconds, comes back as `"1700000000000000"`, and
            // then fails `hex` with "not a hex literal": a table that reads
            // perfectly and cannot be updated, deleted or fetched by id.
            Some(ty) if ty.is_identity() || ty.is_connection_id() => Self::hex(id),
            Some(ty) if ty.is_timestamp() || ty.is_time_duration() => {
                id.parse::<i128>().map(Self::integer).map_err(|_| {
                    error!(
                        "id is not a microsecond count, but its column is a time",
                        id = id.to_string()
                    )
                })
            }
            _ => Self::text(id),
        }
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// One `WHERE` comparison.
#[derive(Debug, Clone)]
pub(crate) struct Cond {
    column: Ident,
    op: &'static str,
    value: Literal,
}

impl Cond {
    pub(crate) fn new(column: Ident, op: FilterOp, value: Literal) -> Result<Self> {
        Ok(Self {
            column,
            op: sql_operator(op)?,
            value,
        })
    }

    /// An equality test, for the id comparisons every single-row statement makes.
    pub(crate) fn eq(column: Ident, value: Literal) -> Self {
        Self {
            column,
            op: "=",
            value,
        }
    }

    fn render(&self) -> String {
        format!(
            "{} {} {}",
            self.column.as_str(),
            self.op,
            self.value.as_str()
        )
    }
}

/// The `WHERE` shared by every statement kind.
///
/// Single-row writes used to build an id-only `WHERE` of their own while reads
/// appended the vista's conditions, so a narrowed vista could modify rows it
/// could not see. One renderer removes the opportunity to forget.
fn render_where(conditions: &[Cond]) -> String {
    if conditions.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = conditions.iter().map(Cond::render).collect();
    format!(" WHERE {}", parts.join(" AND "))
}

/// What a `SELECT` asks for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Projection {
    All,
    /// `COUNT(*) AS n` — the one aggregate the dialect offers.
    CountAs(&'static str),
}

#[derive(Debug, Clone)]
pub(crate) struct Select {
    relation: Ident,
    projection: Projection,
    conditions: Vec<Cond>,
    limit: Option<u64>,
}

impl Select {
    pub(crate) fn new(relation: Ident) -> Self {
        Self {
            relation,
            projection: Projection::All,
            conditions: Vec::new(),
            limit: None,
        }
    }

    pub(crate) fn count_as(mut self, alias: &'static str) -> Self {
        self.projection = Projection::CountAs(alias);
        self
    }

    pub(crate) fn where_all(mut self, conditions: impl IntoIterator<Item = Cond>) -> Self {
        self.conditions.extend(conditions);
        self
    }

    pub(crate) fn and_where(mut self, condition: Cond) -> Self {
        self.conditions.push(condition);
        self
    }

    /// `LIMIT` is the one clause of its family the dialect has — there is no
    /// `OFFSET`, which is why the capabilities refuse paging.
    pub(crate) fn limit(mut self, rows: u64) -> Self {
        self.limit = Some(rows);
        self
    }

    pub(crate) fn render(&self) -> String {
        let projection = match self.projection {
            Projection::All => "*".to_string(),
            Projection::CountAs(alias) => format!("COUNT(*) AS {alias}"),
        };
        format!(
            "SELECT {} FROM {}{}{}",
            projection,
            self.relation.as_str(),
            render_where(&self.conditions),
            match self.limit {
                Some(n) => format!(" LIMIT {n}"),
                None => String::new(),
            }
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Insert {
    relation: Ident,
    columns: Vec<Ident>,
    values: Vec<Literal>,
}

impl Insert {
    pub(crate) fn new(relation: Ident) -> Self {
        Self {
            relation,
            columns: Vec::new(),
            values: Vec::new(),
        }
    }

    pub(crate) fn set(mut self, column: Ident, value: Literal) -> Self {
        self.columns.push(column);
        self.values.push(value);
        self
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub(crate) fn render(&self) -> String {
        let columns: Vec<&str> = self.columns.iter().map(Ident::as_str).collect();
        let values: Vec<&str> = self.values.iter().map(Literal::as_str).collect();
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            self.relation.as_str(),
            columns.join(", "),
            values.join(", ")
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Update {
    relation: Ident,
    assignments: Vec<(Ident, Literal)>,
    conditions: Vec<Cond>,
}

impl Update {
    pub(crate) fn new(relation: Ident) -> Self {
        Self {
            relation,
            assignments: Vec::new(),
            conditions: Vec::new(),
        }
    }

    pub(crate) fn set(mut self, column: Ident, value: Literal) -> Self {
        self.assignments.push((column, value));
        self
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.assignments.is_empty()
    }

    pub(crate) fn where_all(mut self, conditions: impl IntoIterator<Item = Cond>) -> Self {
        self.conditions.extend(conditions);
        self
    }

    pub(crate) fn and_where(mut self, condition: Cond) -> Self {
        self.conditions.push(condition);
        self
    }

    pub(crate) fn render(&self) -> String {
        let assignments: Vec<String> = self
            .assignments
            .iter()
            .map(|(column, value)| format!("{} = {}", column.as_str(), value.as_str()))
            .collect();
        format!(
            "UPDATE {} SET {}{}",
            self.relation.as_str(),
            assignments.join(", "),
            render_where(&self.conditions)
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Delete {
    relation: Ident,
    conditions: Vec<Cond>,
}

impl Delete {
    pub(crate) fn new(relation: Ident) -> Self {
        Self {
            relation,
            conditions: Vec::new(),
        }
    }

    pub(crate) fn where_all(mut self, conditions: impl IntoIterator<Item = Cond>) -> Self {
        self.conditions.extend(conditions);
        self
    }

    pub(crate) fn and_where(mut self, condition: Cond) -> Self {
        self.conditions.push(condition);
        self
    }

    pub(crate) fn render(&self) -> String {
        format!(
            "DELETE FROM {}{}",
            self.relation.as_str(),
            render_where(&self.conditions)
        )
    }
}

/// The comparison operators the dialect accepts.
pub(crate) fn sql_operator(op: FilterOp) -> Result<&'static str> {
    Ok(match op {
        FilterOp::Eq => "=",
        FilterOp::Ne => "!=",
        FilterOp::Gt => ">",
        FilterOp::Gte => ">=",
        FilterOp::Lt => "<",
        FilterOp::Lte => "<=",
        FilterOp::InSet | FilterOp::NotInSet => {
            return Err(error!("no `IN` operator in SpacetimeDB SQL").mark_unimplemented());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(name: &str) -> Ident {
        Ident::new(name).expect("a plain name is valid")
    }

    #[test]
    fn plain_names_are_bare_and_odd_ones_quoted() {
        assert_eq!(ident("game_id").as_str(), "game_id");
        assert_eq!(ident("odd name").as_str(), "\"odd name\"");
    }

    #[test]
    fn an_identifier_carrying_a_quote_is_refused_not_escaped() {
        // No SpacetimeDB schema declares one, so this means the schema is not
        // what we think it is — escaping would paper over that.
        assert!(Ident::new("na\"me").is_err());
        assert!(Ident::new("na\nme").is_err());
        assert!(Ident::new("").is_err());
    }

    #[test]
    fn string_literals_double_embedded_quotes() {
        assert_eq!(Literal::text("O'Hara").unwrap().as_str(), "'O''Hara'");
        // The classic breakout closes the quote and appends a statement; doubling
        // keeps the whole thing one inert string.
        assert_eq!(
            Literal::text("'; DROP TABLE account; --").unwrap().as_str(),
            "'''; DROP TABLE account; --'"
        );
    }

    #[test]
    fn a_nul_in_a_string_is_refused() {
        // Quote doubling survives it, but a NUL can truncate the statement at
        // any C boundary the request crosses.
        assert!(Literal::text("ok\0DROP").is_err());
    }

    #[test]
    fn hex_literals_must_be_hex_all_the_way() {
        assert_eq!(Literal::hex("0xdeadBEEF").unwrap().as_str(), "0xdeadBEEF");
        // The regression this module exists for: `0x` used to be enough to have
        // the rest of the string emitted unquoted, straight into the statement.
        assert!(Literal::hex("0x0 OR 1=1; DELETE FROM account").is_err());
        assert!(Literal::hex("0x").is_err());
        assert!(Literal::hex("deadbeef").is_err());
    }

    #[test]
    fn an_id_is_rendered_for_its_declared_column_type() {
        // Numeric column, numeric id: bare, so it matches a u64 key.
        assert_eq!(
            Literal::for_id(Some(&AlgebraicType::U64), "42")
                .unwrap()
                .as_str(),
            "42"
        );
        // Text column, numeric-looking id: still quoted. Guessing from the shape
        // emitted this bare, and it matched nothing.
        assert_eq!(
            Literal::for_id(Some(&AlgebraicType::String), "42")
                .unwrap()
                .as_str(),
            "'42'"
        );
        // Text column, hex-looking id: likewise quoted rather than injected.
        assert_eq!(
            Literal::for_id(Some(&AlgebraicType::String), "0x1 OR 1=1")
                .unwrap()
                .as_str(),
            "'0x1 OR 1=1'"
        );
        // Unknown column: quote, because a quoted literal can only fail to
        // match, while a bare one can change the statement.
        assert_eq!(Literal::for_id(None, "42").unwrap().as_str(), "'42'");
        // Numeric column, non-numeric id: refused rather than rendered.
        assert!(Literal::for_id(Some(&AlgebraicType::U64), "not a number").is_err());
    }

    #[test]
    fn every_special_type_renders_an_id_the_way_it_was_read() {
        // These five are what `AlgebraicType::is_special()` covers, and each has
        // to come back the way `value::special_product_to_cbor` sent it out. The
        // pairing is the test: an id is a string this driver produced from a row,
        // so a mismatch here is a row that lists and can never be addressed.
        //
        // Sending all five to `hex` — which `is_special()` reads as if it invites
        // — is what broke it: a `Timestamp` reads out as microseconds and then
        // failed to render back with "not a hex literal".
        let hex = "0xdeadbeef";
        assert_eq!(
            Literal::for_id(Some(&AlgebraicType::identity()), hex)
                .unwrap()
                .as_str(),
            hex,
            "identity reads as hex, so it must render as hex"
        );
        assert_eq!(
            Literal::for_id(Some(&AlgebraicType::connection_id()), hex)
                .unwrap()
                .as_str(),
            hex
        );
        assert_eq!(
            Literal::for_id(Some(&AlgebraicType::timestamp()), "1700000000000000")
                .unwrap()
                .as_str(),
            "1700000000000000",
            "a timestamp reads as microseconds, so it must render as a number"
        );
        assert_eq!(
            Literal::for_id(Some(&AlgebraicType::time_duration()), "-500")
                .unwrap()
                .as_str(),
            "-500"
        );
        // A u128 is too wide for CBOR's integer, so it reads as text and renders
        // back quoted.
        assert_eq!(
            Literal::for_id(
                Some(&AlgebraicType::uuid()),
                "340282366920938463463374607431768211455"
            )
            .unwrap()
            .as_str(),
            "'340282366920938463463374607431768211455'"
        );
        // And a time column still refuses an id that is not a number, rather than
        // emitting it bare into the statement.
        assert!(Literal::for_id(Some(&AlgebraicType::timestamp()), "0x1; DROP").is_err());
    }

    #[test]
    fn a_select_carries_its_conditions_and_limit() {
        let sql = Select::new(ident("game"))
            .and_where(Cond::eq(ident("status"), Literal::text("playing").unwrap()))
            .limit(1)
            .render();
        assert_eq!(sql, "SELECT * FROM game WHERE status = 'playing' LIMIT 1");
    }

    #[test]
    fn count_uses_the_one_aggregate_the_dialect_has() {
        let sql = Select::new(ident("game")).count_as("n").render();
        assert_eq!(sql, "SELECT COUNT(*) AS n FROM game");
    }

    #[test]
    fn a_single_row_write_keeps_the_vistas_conditions() {
        // The narrowing that reads apply must bind writes too, or a vista can
        // change rows it cannot see.
        let narrowing = Cond::eq(ident("game_id"), Literal::integer(7));
        let update = Update::new(ident("seat"))
            .set(ident("chips"), Literal::integer(100))
            .and_where(Cond::eq(ident("seat_id"), Literal::integer(3)))
            .where_all([narrowing.clone()])
            .render();
        assert_eq!(
            update,
            "UPDATE seat SET chips = 100 WHERE seat_id = 3 AND game_id = 7"
        );

        let delete = Delete::new(ident("seat"))
            .and_where(Cond::eq(ident("seat_id"), Literal::integer(3)))
            .where_all([narrowing])
            .render();
        assert_eq!(delete, "DELETE FROM seat WHERE seat_id = 3 AND game_id = 7");
    }

    #[test]
    fn in_is_refused_because_the_dialect_has_no_such_operator() {
        assert!(sql_operator(FilterOp::InSet).is_err());
        assert!(sql_operator(FilterOp::NotInSet).is_err());
        assert_eq!(sql_operator(FilterOp::Gte).unwrap(), ">=");
    }
}
